// Pure matching rules for the fcitx5 OpenLess hotkeys.
//
// Why this exists (measured, not guessed):
//   fcitx5 hands the addon the *level-applied* key symbol — pressing
//   Ctrl+Shift+; arrives as sym=0x3a (':') on the Wayland frontend, while the
//   binding we registered was built from the base symbol 0x3b (';').
//   fcitx::Key::normalize() only folds the letter case (a-z -> A-Z) and drops
//   Shift for *symbols*; it does not map ';' to ':'. An exact
//   `sym == registered && states == registered` test therefore never fires for
//   any Shift+symbol shortcut (Ctrl+Shift+; and Ctrl+Shift+S were both dead).
//
// So compare on a folded pair: letters case-insensitively, US-layout
// base/shifted symbols as the same physical key, and allow the Shift bit to
// differ *only* when the two symbols are such a pair. That keeps Ctrl+; and
// Ctrl+Shift+; apart while accepting either frontend convention.
#pragma once

#include <cstdint>

#include <fcitx-utils/key.h>

namespace openless_hotkeys {

constexpr uint32_t kShiftBit = 0x01;

/// X11 modifier keysyms (Shift_L … Hyper_R) plus CapsLock/ShiftLock.
inline bool isModifierSym(uint32_t sym) { return sym >= 0xffe1 && sym <= 0xffee; }

/// US-layout base <-> shifted symbol pairs. Mirrors the host's table in
/// `linux-egui/src/settings.rs::primary_keysym`; keep both in sync.
inline bool isShiftPair(uint32_t left, uint32_t right) {
    if (left == right) {
        return false;
    }
    static constexpr uint32_t kPairs[][2] = {
        {';', ':'}, {',', '<'}, {'.', '>'}, {'/', '?'}, {'\\', '|'},
        {'[', '{'}, {']', '}'}, {'\'', '"'}, {'`', '~'}, {'-', '_'},
        {'=', '+'}, {'1', '!'}, {'2', '@'}, {'3', '#'}, {'4', '$'},
        {'5', '%'}, {'6', '^'}, {'7', '&'}, {'8', '*'}, {'9', '('},
        {'0', ')'},
    };
    for (const auto &pair : kPairs) {
        if ((left == pair[0] && right == pair[1]) ||
            (left == pair[1] && right == pair[0])) {
            return true;
        }
    }
    return false;
}

/// Fold a symbol the way fcitx's own normalization does for letters: the
/// frontend may report either 'a' or 'A' for the same physical key.
inline uint32_t foldSym(uint32_t sym) {
    if (sym >= 'a' && sym <= 'z') {
        return sym - 32;
    }
    return sym;
}

inline bool symMatches(uint32_t eventSym, uint32_t registeredSym) {
    const uint32_t event = foldSym(eventSym);
    const uint32_t registered = foldSym(registeredSym);
    if (event == registered) {
        return true;
    }
    // A bare modifier key must never be folded into another key.
    if (isModifierSym(event) || isModifierSym(registered)) {
        return false;
    }
    return isShiftPair(event, registered);
}

/// The modifier mask must match exactly.
///
/// Measured: for a *symbol* key fcitx keeps the Shift bit in `states` and only
/// `Key::normalize()` drops it, while for letters it uppercases the symbol and
/// keeps the bit. Tolerating a differing Shift bit here would make Ctrl+; fire
/// a Ctrl+Shift+; binding, which is worse than the edge case it would cover.
inline bool statesMatch(uint32_t eventStates, uint32_t registeredStates) {
    return eventStates == registeredStates;
}

inline bool matches(uint32_t eventSym, uint32_t eventStates,
                    uint32_t registeredSym, uint32_t registeredStates) {
    if (registeredSym == 0) {
        return false;
    }
    return symMatches(eventSym, registeredSym) &&
           statesMatch(eventStates, registeredStates);
}

/// A matched binding whose primary key is a modifier must never be consumed:
/// swallowing Shift_L/Ctrl_L makes the modifier vanish for every application
/// (pressing Shift+letter stopped producing uppercase system-wide). Those
/// bindings are observed only — the host still gets press/release events.
inline bool shouldConsume(uint32_t registeredSym) {
    return !isModifierSym(registeredSym);
}

} // namespace openless_hotkeys
