// Contract for the hotkey matching table. Pure functions, no fcitx5 instance,
// no display: every case below was measured against fcitx::Key's own
// normalization before it was encoded here.
#include "hotkey_match.h"

#include <cassert>
#include <cstdio>
#include <vector>

using openless_hotkeys::matches;
using openless_hotkeys::shouldConsume;
using openless_hotkeys::symMatches;

static constexpr uint32_t kCtrl = 0x04;
static constexpr uint32_t kAlt = 0x08;
static constexpr uint32_t kShift = 0x01;

static void expect(bool actual, const char *what) {
    if (!actual) {
        std::fprintf(stderr, "FAIL: %s\n", what);
        assert(false);
    }
    std::printf("ok: %s\n", what);
}

int main() {
    // 1. The reported bug: Ctrl+Shift+; registered as ';' + Shift must fire
    //    when the frontend reports the level-applied ':' symbol.
    expect(matches(':', kCtrl | kShift, ';', kCtrl | kShift),
           "Ctrl+Shift+: matches registered ;+Ctrl+Shift (level-applied frontend)");
    expect(matches(';', kCtrl | kShift, ';', kCtrl | kShift),
           "Ctrl+Shift+; matches registered ;+Ctrl+Shift (unfolded frontend)");
    expect(matches(':', kCtrl, ';', kCtrl | kShift) == false,
           "Ctrl+: does not fire the Ctrl+Shift+; binding");
    expect(matches(';', kCtrl, ';', kCtrl | kShift) == false,
           "Ctrl+; does not fire the Ctrl+Shift+; binding");

    // 2. Registration in the shifted form (host may pick either spelling).
    expect(matches(':', kCtrl | kShift, ':', kCtrl | kShift),
           "shifted spelling matches itself");
    expect(matches(';', kCtrl | kShift, ':', kCtrl | kShift),
           "base symbol matches a shifted registration");

    // 3. Letters: fcitx reports A-Z when a modifier is held, a-z otherwise.
    expect(matches('S', kCtrl | kShift, 's', kCtrl | kShift),
           "Ctrl+Shift+S matches registered s+Ctrl+Shift");
    expect(matches('s', kCtrl | kShift, 's', kCtrl | kShift),
           "lowercase event matches registered s+Ctrl+Shift");
    expect(matches('A', kAlt, 'a', kAlt), "Alt+A matches registered a+Alt");
    expect(matches('A', kAlt, 's', kAlt) == false, "Alt+A never matches s+Alt");

    // 4. Other symbol pairs on the same physical key.
    expect(matches('?', kCtrl | kShift, '/', kCtrl | kShift),
           "Ctrl+Shift+? matches registered /+Ctrl+Shift");
    expect(symMatches('?', '/') && symMatches('/', '?'),
           "symbol pair is symmetric");
    expect(symMatches(';', ';') && symMatches('/', '/'),
           "identical symbols match");
    expect(symMatches(';', '/') == false, "different symbols do not match");

    // 5. Bare modifiers are matched exactly and never folded into anything.
    expect(matches(0xffe3, 0, 0xffe3, 0),
           "bare Left Control matches its own registration");
    expect(matches(0xffe3, 0, 0xffe1, 0) == false,
           "Left Control never matches Shift");
    expect(matches(0xffe3, kCtrl, 0xffe3, 0) == false,
           "modifier with a stale modifier bit does not match");
    expect(shouldConsume(0xffe3) == false && shouldConsume(0xffe1) == false,
           "modifier-only bindings are never consumed");
    expect(shouldConsume(';') && shouldConsume(0xff0d),
           "normal keys are still consumed");

    // 6. Functional keys and empty registrations.
    expect(matches(0xff0d, kCtrl | kShift, 0xff0d, kCtrl | kShift),
           "Enter binding matches");
    expect(matches(0xff0d, kCtrl | kShift, 0xff0d, kCtrl) == false,
           "Enter does not match with a different modifier set");
    expect(matches(';', kCtrl, 0, 0) == false, "unregistered slot never matches");

    std::printf("hotkey_match contract passed\n");
    return 0;
}
