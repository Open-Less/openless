// Exercise the real plugin entry points with two in-process native input
// contexts. No display, user keyboard, clipboard contents or DBus service is
// required; only the input-context boundary is replaced by a recording client.
// The compositor-side primary-selection probe is injected the same way (see
// OpenLessInputTargetContract::setPrimaryReader), so the assertions never depend
// on the desktop's current selection.
#include "openless.cpp"
#include <cassert>
#include <filesystem>
#include <unistd.h>

using PrimarySelectionStatus = openless_selection::PrimarySelectionStatus;

namespace {
openless_selection::PrimarySelectionSnapshot primarySnapshot(
    PrimarySelectionStatus status, std::string text = std::string(),
    std::string detail = std::string()) {
    openless_selection::PrimarySelectionSnapshot snapshot;
    snapshot.status = status;
    snapshot.text = std::move(text);
    snapshot.detail = std::move(detail);
    return snapshot;
}
} // namespace

class RecordingInputContext final : public fcitx::InputContext {
public:
    explicit RecordingInputContext(fcitx::InputContextManager &manager)
        : InputContext(manager, "openless-contract") { created(); }
    ~RecordingInputContext() override { destroy(); }
    const char *frontend() const override { return "contract"; }
    std::vector<std::string> committed;
protected:
    void commitStringImpl(const std::string &text) override { committed.push_back(text); }
    void deleteSurroundingTextImpl(int, unsigned int) override {}
    void forwardKeyImpl(const fcitx::ForwardKeyEvent &) override {}
    void updatePreeditImpl() override {}
};

namespace fcitx {
struct OpenLessInputTargetContract {
    static void select(OpenLess &plugin, InputContext &context) { plugin.selectionIc_ = &context; }
    static void type(OpenLess &plugin, InputContext &context) { plugin.savedIc_ = &context; }
    static void setPrimaryReader(
        OpenLess &plugin,
        openless_selection::PrimarySelectionSnapshot snapshot) {
        plugin.primarySelectionReader_ =
            [snapshot = std::move(snapshot)]() { return snapshot; };
    }
    /// 注入 clipboard addon 缓存的内容（只影响“探针不可用”时的最后一级兜底）。
    static void setSelectionCacheReader(OpenLess &plugin, std::string text) {
        plugin.selectionCacheReader_ = [text = std::move(text)]() { return text; };
    }
};
}

int main() {
    const auto config = std::filesystem::temp_directory_path() /
        ("openless-fcitx-contract-" + std::to_string(getpid()));
    std::filesystem::create_directory(config);
    setenv("XDG_CONFIG_HOME", config.c_str(), 1);
    {
        char name[] = "openless-contract";
        char disabled[] = "--disable=all";
        char *arguments[] = {name, disabled, nullptr};
        fcitx::Instance instance(2, arguments);
        instance.initialize();
        fcitx::OpenLess plugin(&instance);
        // 契约用例不接触真实合成器：默认注入"探针给不出结论"，让旧用例只走应用
        // surrounding 这条路（与历史行为一致，也不会被桌面此刻的选区影响）。
        fcitx::OpenLessInputTargetContract::setPrimaryReader(
            plugin, openless_selection::PrimarySelectionSnapshot{});
        RecordingInputContext first(instance.inputContextManager());
        first.focusIn();
        fcitx::OpenLessInputTargetContract::select(plugin, first);
        first.surroundingText().setText("foo foo", 3, 0);
        assert(plugin.captureSelectionTarget("selection") == "foo");
        const auto metadata=plugin.contextSnapshot("selection",false);
        assert(metadata.find("openless-contract")!=std::string::npos);
        assert(metadata.find("\"text\"")==std::string::npos);
        assert(plugin.contextSnapshot("selection",true).find("foo foo")!=std::string::npos);
        first.setCapabilityFlags(fcitx::CapabilityFlag::Password);
        assert(plugin.captureSelectionTarget("password").empty());
        assert(plugin.contextSnapshot("selection",true).find("\"text\"")==std::string::npos);
        first.setCapabilityFlags({});
        // Identical text at a different position is a different selection.
        // Comparing only the selected string would corrupt the wrong range.
        first.surroundingText().setCursor(7, 4);
        assert(!plugin.applySelectionTarget("selection", "foo", "replacement"));
        assert(first.committed.empty());
        first.surroundingText().setCursor(3, 3);
        assert(!plugin.applySelectionTarget("selection", "foo", "replacement"));
        // Changing unselected context or invalidating the native snapshot must
        // also invalidate Apply, even while PRIMARY still contains "foo".
        first.surroundingText().setText("foo bar", 3, 0);
        assert(!plugin.applySelectionTarget("selection", "foo", "replacement"));
        first.surroundingText().invalidate();
        assert(!plugin.applySelectionTarget("selection", "foo", "replacement"));
        first.surroundingText().setText("foo foo", 3, 0);
        // The QA -> preview handoff moves the captured values, never the live
        // SurroundingText object. The old ticket must become unusable.
        assert(plugin.rekeySelectionTarget("selection", "preview"));
        assert(!plugin.applySelectionTarget("selection", "foo", "replacement"));
        assert(plugin.applySelectionTarget("preview", "foo", "replacement"));
        assert(first.committed == std::vector<std::string>{"replacement"});
        // A client reports its new surrounding text after commit. Undo still
        // uses the captured offsets after the ticket has been transferred.
        first.surroundingText().setText("replacement foo", 11, 11);
        assert(plugin.revertSelectionTarget("preview"));
        assert(first.committed.back() == "foo");
        assert(!plugin.revertSelectionTarget("preview"));

        RecordingInputContext second(instance.inputContextManager());
        fcitx::OpenLessInputTargetContract::type(plugin, first);
        assert(plugin.captureDictationTarget("dictation"));
        first.focusOut();
        second.focusIn();
        fcitx::OpenLessInputTargetContract::type(plugin, second);
        assert(!plugin.commitDictationTarget("dictation", "focus changed"));
        second.focusOut();
        first.focusIn();
        assert(plugin.commitDictationTarget("dictation", "original target"));
        assert(first.committed.back() == "original target");
        assert(second.committed.empty());
        assert(plugin.cancelDictationTarget("dictation"));
        assert(!plugin.commitDictationTarget("dictation", "late write"));

        {
            RecordingInputContext destroyed(instance.inputContextManager());
            first.focusOut();
            destroyed.focusIn();
            destroyed.surroundingText().setText("original", 8, 0);
            fcitx::OpenLessInputTargetContract::type(plugin, destroyed);
            fcitx::OpenLessInputTargetContract::select(plugin, destroyed);
            assert(plugin.captureDictationTarget("destroyed-dictation"));
            assert(plugin.captureSelectionTarget("destroyed-selection") == "original");
        }
        // Destruction emits the actual InputContextDestroyed event. Both
        // ticket maps must drop the raw handle before either late write runs.
        assert(!plugin.commitDictationTarget("destroyed-dictation", "late write"));
        assert(!plugin.applySelectionTarget("destroyed-selection", "original", "late write"));

        // ---- 选区新鲜度（②）与来源优先级（①③）----
        // ① 探针读到文本：即使应用报的是另一段文本，也用探针的（③ 不再无条件优先 surrounding）。
        {
            RecordingInputContext fresh(instance.inputContextManager());
            fresh.surroundingText().setText("stale app selection", 19, 0);
            fcitx::OpenLessInputTargetContract::select(plugin, fresh);
            fcitx::OpenLessInputTargetContract::setPrimaryReader(
                plugin, primarySnapshot(PrimarySelectionStatus::Text, "fresh primary"));
            assert(plugin.captureSelectionTarget("fresh-primary") == "fresh primary");
            assert(plugin.getSelectionText() == "fresh primary");
            assert(plugin.cancelSelectionTarget("fresh-primary"));
        }
        // ② 新选区没有 text mime（图片/文件/密码）→ 判失效：不退回 clipboard 缓存，
        //    也不退回 surrounding，否则"上一次的选区文本"会重新变成当前选区。
        {
            fcitx::OpenLessInputTargetContract::setPrimaryReader(
                plugin,
                primarySnapshot(PrimarySelectionStatus::NoText, std::string(),
                                "image/png, text/html"));
            assert(plugin.getSelectionText().empty());
            RecordingInputContext imageSelection(instance.inputContextManager());
            imageSelection.surroundingText().setText("previous selection", 18, 0);
            fcitx::OpenLessInputTargetContract::select(plugin, imageSelection);
            assert(plugin.captureSelectionTarget("no-text").empty());
            assert(!plugin.applySelectionTarget("no-text", "previous selection", "x"));
            // 没捕获到目标，也就没有 ticket 可释放（这不是错误路径）。
            assert(!plugin.cancelSelectionTarget("no-text"));
        }
        // 探针不可用（X11 会话 / 合成器没有 ext-data-control）→ 退回应用 surrounding。
        {
            RecordingInputContext fallback(instance.inputContextManager());
            fallback.surroundingText().setText("foo foo", 3, 0);
            fcitx::OpenLessInputTargetContract::select(plugin, fallback);
            fcitx::OpenLessInputTargetContract::setPrimaryReader(
                plugin, primarySnapshot(PrimarySelectionStatus::Unsupported, std::string(),
                                       "compositor has no ext_data_control_manager_v1"));
            assert(plugin.captureSelectionTarget("probe-unavailable") == "foo");
            assert(plugin.cancelSelectionTarget("probe-unavailable"));
        }
        // 合成器明确说没有 PRIMARY 选区（应用从不导出 PRIMARY）→ 仍用 surrounding。
        {
            RecordingInputContext noPrimary(instance.inputContextManager());
            noPrimary.surroundingText().setText("bar bar", 3, 0);
            fcitx::OpenLessInputTargetContract::select(plugin, noPrimary);
            fcitx::OpenLessInputTargetContract::setPrimaryReader(
                plugin, primarySnapshot(PrimarySelectionStatus::NoSelection));
            assert(plugin.getSelectionText().empty());
            assert(plugin.captureSelectionTarget("no-primary") == "bar");
            assert(plugin.cancelSelectionTarget("no-primary"));
        }
        // 数据没读完（来源应用卡住）→ 用 surrounding，仍然不碰陈旧缓存。
        {
            RecordingInputContext readFailed(instance.inputContextManager());
            readFailed.surroundingText().setText("baz baz", 3, 0);
            fcitx::OpenLessInputTargetContract::select(plugin, readFailed);
            fcitx::OpenLessInputTargetContract::setPrimaryReader(
                plugin, primarySnapshot(PrimarySelectionStatus::ReadFailed, std::string(),
                                       "text/plain (read timed out)"));
            assert(plugin.captureSelectionTarget("read-failed") == "baz");
            assert(plugin.cancelSelectionTarget("read-failed"));
        }
        // 边界 1：探针**不可用**（X11 会话 / 没有 ext-data-control）+ 应用 surrounding 也为空
        //         → 最后一级才回退 clipboard 缓存（避免无 surrounding 的 XIM 类应用在
        //         X11 会话下彻底拿不到选区）。
        {
            RecordingInputContext x11Context(instance.inputContextManager());
            fcitx::OpenLessInputTargetContract::select(plugin, x11Context);
            fcitx::OpenLessInputTargetContract::setPrimaryReader(
                plugin, primarySnapshot(PrimarySelectionStatus::Unsupported, std::string(),
                                        "WAYLAND_DISPLAY is not set"));
            fcitx::OpenLessInputTargetContract::setSelectionCacheReader(plugin,
                                                                        "cached primary");
            assert(plugin.captureSelectionTarget("cache-fallback") == "cached primary");
            assert(plugin.cancelSelectionTarget("cache-fallback"));
            assert(plugin.getSelectionText() == "cached primary");
        }
        // 边界 2：探针**给出了结论**（NoSelection / NoText）→ 严格判失效，绝不碰缓存。
        {
            RecordingInputContext conclusive(instance.inputContextManager());
            fcitx::OpenLessInputTargetContract::select(plugin, conclusive);
            fcitx::OpenLessInputTargetContract::setSelectionCacheReader(plugin,
                                                                        "cached primary");
            fcitx::OpenLessInputTargetContract::setPrimaryReader(
                plugin, primarySnapshot(PrimarySelectionStatus::NoSelection, std::string(),
                                        "no primary selection"));
            assert(plugin.captureSelectionTarget("no-selection-no-cache").empty());
            assert(plugin.getSelectionText().empty());
            fcitx::OpenLessInputTargetContract::setPrimaryReader(
                plugin, primarySnapshot(PrimarySelectionStatus::NoText, std::string(),
                                        "image/png"));
            assert(plugin.captureSelectionTarget("no-text-no-cache").empty());
            assert(plugin.getSelectionText().empty());
        }
        // 纯策略补充：探针读到空文本时用 surrounding（探针没能给出文本，不是"选中的不是文本"）。
        assert(openless_selection::chooseSelectionSource(
                   primarySnapshot(PrimarySelectionStatus::Text, std::string()),
                   "surrounding text")
                   .text == "surrounding text");
        assert(openless_selection::chooseSelectionSource(
                   primarySnapshot(PrimarySelectionStatus::NoText),
                   "surrounding text")
                   .text.empty());
    }
    std::filesystem::remove_all(config);
}
