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

/// The only modifier bits fcitx5 keeps after `Key::normalize()` (see
/// fcitx-utils/keysym.h: Shift 1<<0, Ctrl 1<<2, Alt 1<<3, Super 1<<6).
/// CapsLock (1<<1), NumLock (1<<4), Hyper/Mod3, Mod5 and the Gtk virtual
/// Super2/Hyper2/Meta bits are *not* part of a shortcut's identity: they ride
/// along on every key event and must not decide whether a hotkey fires.
constexpr uint32_t kModifierMask = 0x01u | 0x04u | 0x08u | 0x40u;

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

/// The four modifier bits must match exactly; every other state bit is noise.
///
/// Measured (trace, KDE/Wayland + this addon): whether Shift shows up in
/// `states` depends on the key kind — letters keep it (`Ctrl+Shift+S` arrives
/// as sym=0x53 states=0x05), symbols do not (`Ctrl+Shift+;` arrives as
/// sym=0x3a states=0x04, Shift folded into the level-applied symbol).
/// `matches` therefore treats the Shift bit as part of the identity only when
/// the symbol itself cannot distinguish the two (see `matches`).
///
/// Measured (CapsLock): with CapsLock on, every event carries `0x02` in
/// `states`, so `Ctrl+Shift+;` arrived as 0x07 while the binding was registered
/// as 0x05 — an exact comparison never fired for *any* shortcut, and the
/// near-miss logger (which filtered on the same equality) stayed silent too.
/// Masking to the modifier bits keeps Lock/NumLock/Mod3/Mod5 out of the
/// decision while leaving Shift, Ctrl, Alt and Super exact.
inline bool statesMatch(uint32_t eventStates, uint32_t registeredStates) {
    return (eventStates & kModifierMask) == (registeredStates & kModifierMask);
}

/// Match a binding across both frontend conventions for Shift.
///
/// Measured on this desktop (plugin trace enabled, injected and real keys):
///   Ctrl+Shift+S -> sym=0x53 ('S') states=0x05  (Shift is in `states`)
///   Ctrl+Shift+; -> sym=0x3a (':') states=0x04  (Shift folded into the symbol)
/// The registration for `Ctrl+Shift+;` is sym=0x3b states=0x05, so a plain
/// `symMatches && statesMatch` never fired — the shortcut was dead while the
/// key *was* reaching the addon. Rules here:
///   * same symbol → every modifier bit must match (this is what keeps letter
///     bindings, and unshifted symbol bindings, exact);
///   * base/shifted symbol pair → only meaningful when the *binding* asks for
///     Shift; the event's Shift bit is then ignored, because the level-applied
///     symbol already carries that information. A binding that does not ask for
///     Shift is never satisfied by the shifted symbol, so Ctrl+; and
///     Ctrl+Shift+; stay distinguishable in both directions.
inline bool matches(uint32_t eventSym, uint32_t eventStates,
                    uint32_t registeredSym, uint32_t registeredStates) {
    if (registeredSym == 0) {
        return false;
    }
    const uint32_t event = foldSym(eventSym);
    const uint32_t registered = foldSym(registeredSym);
    if (event == registered) {
        return statesMatch(eventStates, registeredStates);
    }
    if (isModifierSym(event) || isModifierSym(registered)) {
        return false;
    }
    if (!isShiftPair(event, registered)) {
        return false;
    }
    if ((registeredStates & kShiftBit) == 0) {
        return false;
    }
    const uint32_t mask = kModifierMask & ~kShiftBit;
    return (eventStates & mask) == (registeredStates & mask);
}

/// A matched binding whose primary key is a modifier must never be consumed:
/// swallowing Shift_L/Ctrl_L makes the modifier vanish for every application
/// (pressing Shift+letter stopped producing uppercase system-wide). Those
/// bindings are observed only — the host still gets press/release events.
inline bool shouldConsume(uint32_t registeredSym) {
    return !isModifierSym(registeredSym);
}

} // namespace openless_hotkeys
