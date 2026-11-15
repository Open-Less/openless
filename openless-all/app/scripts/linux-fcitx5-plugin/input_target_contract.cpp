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
        // 选区读取只走 fcitx5 clipboard addon 的 PRIMARY（见 captureSelectionTarget
        // 的注释）。契约用例不接触真实剪贴板，用注入值代替 addon 的返回。
        fcitx::OpenLessInputTargetContract::setSelectionCacheReader(plugin, "foo");
        RecordingInputContext first(instance.inputContextManager());
        fcitx::OpenLessInputTargetContract::select(plugin, first);
        first.surroundingText().setText("foo foo", 3, 0);
        assert(plugin.captureSelectionTarget("selection") == "foo");
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
        fcitx::OpenLessInputTargetContract::type(plugin, second);
        assert(plugin.commitDictationTarget("dictation", "original target"));
        assert(first.committed.back() == "original target");
        assert(second.committed.empty());
        assert(plugin.cancelDictationTarget("dictation"));
        assert(!plugin.commitDictationTarget("dictation", "late write"));

        {
            fcitx::OpenLessInputTargetContract::setSelectionCacheReader(plugin, "original");
            RecordingInputContext destroyed(instance.inputContextManager());
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

        // ---- 选区读取模型：只认 clipboard addon 的 PRIMARY ----
        // 读到什么就捕获什么：不再有「探针 / 缓存 / surrounding」三方抢答。
        {
            RecordingInputContext ctx(instance.inputContextManager());
            ctx.surroundingText().setText("stale app selection", 19, 0);
            fcitx::OpenLessInputTargetContract::select(plugin, ctx);
            fcitx::OpenLessInputTargetContract::setSelectionCacheReader(
                plugin, "fresh primary");
            assert(plugin.captureSelectionTarget("fresh-primary") == "fresh primary");
            assert(plugin.getSelectionText() == "fresh primary");
            assert(plugin.cancelSelectionTarget("fresh-primary"));
        }
        // PRIMARY 为空（图片/文件/密码，或应用根本不导出 PRIMARY）→ 判失效：
        // 既不退回 surrounding，也不退回上一次的文本。
        {
            RecordingInputContext imageSelection(instance.inputContextManager());
            imageSelection.surroundingText().setText("previous selection", 18, 0);
            fcitx::OpenLessInputTargetContract::select(plugin, imageSelection);
            fcitx::OpenLessInputTargetContract::setSelectionCacheReader(plugin, "");
            assert(plugin.getSelectionText().empty());
            assert(plugin.captureSelectionTarget("no-text").empty());
            assert(!plugin.applySelectionTarget("no-text", "previous selection", "x"));
            // 没捕获到目标，也就没有 ticket 可释放（这不是错误路径）。
            assert(!plugin.cancelSelectionTarget("no-text"));
        }
        // 恢复注入值后，同一 ticket 名可以重新捕获（不残留旧状态）。
        {
            fcitx::OpenLessInputTargetContract::setSelectionCacheReader(plugin, "again");
            RecordingInputContext again(instance.inputContextManager());
            again.surroundingText().setText("again again", 3, 0);
            fcitx::OpenLessInputTargetContract::select(plugin, again);
            assert(plugin.captureSelectionTarget("recovered") == "again");
            assert(plugin.cancelSelectionTarget("recovered"));
        }
    }
    std::filesystem::remove_all(config);
}
