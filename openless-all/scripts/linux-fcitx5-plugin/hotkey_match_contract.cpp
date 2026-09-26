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
static constexpr uint32_t kCapsLock = 0x02;
static constexpr uint32_t kNumLock = 0x10;
static constexpr uint32_t kMod3 = 0x20;
static constexpr uint32_t kMod5 = 0x80;

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
    // Measured on the real desktop (plugin trace, KDE/Wayland): pressing
    // Ctrl+Shift+; delivers sym=0x3a (':') with states=0x04 — the Shift bit is
    // NOT in `states` because the frontend folded it into the symbol. This is
    // the case the shortcut was dying on, so it must match.
    expect(matches(':', kCtrl, ';', kCtrl | kShift),
           "Ctrl+Shift+; arrives as ':' with only Ctrl held (measured) and fires");
    expect(matches(':', kCtrl | kCapsLock, ';', kCtrl | kShift),
           "the same arrival still fires while CapsLock rides along");
    // The mirror direction stays strict: the shifted symbol must never satisfy
    // a binding that does not ask for Shift, otherwise Ctrl+; would steal it.
    expect(matches(':', kCtrl, ';', kCtrl) == false,
           "Ctrl+Shift+; does not fire a Ctrl+; binding");
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

    // 7. Lock / virtual state bits are not part of a shortcut's identity.
    //    Measured: with CapsLock on, every key event carried 0x02 on top of the
    //    real modifiers, so the exact comparison this replaced never fired for
    //    *any* binding (the QA shortcut was the visible casualty) — and the
    //    near-miss logger filtered on the same equality, so it stayed silent
    //    and the failure looked like "the key never arrived".
    expect(matches(':', kCtrl | kShift | kCapsLock, ';', kCtrl | kShift),
           "Ctrl+Shift+; still fires while CapsLock is on");
    expect(matches(':', kCtrl | kShift | kNumLock | kMod3 | kMod5, ';', kCtrl | kShift),
           "NumLock/Hyper/Mod5 on the event do not block the binding");
    expect(matches('A', kAlt | kCapsLock, 'a', kAlt),
           "Alt+A still fires while CapsLock is on");
    expect(matches(';', kCtrl | kCapsLock, ';', kCtrl | kShift) == false,
           "CapsLock does not let Ctrl+; fire the Ctrl+Shift+; binding");
    expect(matches(0xffe3, kCapsLock, 0xffe3, 0),
           "a bare modifier binding ignores lock bits");
    expect(openless_hotkeys::statesMatch(kCtrl | kShift | kCapsLock, kCtrl | kShift),
           "statesMatch masks lock bits on both sides");
    expect(openless_hotkeys::statesMatch(kCapsLock, 0),
           "lock bits alone never change the modifier set");
    expect(openless_hotkeys::statesMatch(kCtrl | kShift, kCtrl) == false,
           "statesMatch still distinguishes real modifiers");

    std::printf("hotkey_match contract passed\n");
    return 0;
}
