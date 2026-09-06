// Exercise the real plugin entry points with two in-process native input
// contexts. No display, user keyboard, clipboard contents or DBus service is
// required; only the input-context boundary is replaced by a recording client.
#include "openless.cpp"
#include <cassert>
#include <filesystem>
#include <unistd.h>

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
    }
    std::filesystem::remove_all(config);
}
