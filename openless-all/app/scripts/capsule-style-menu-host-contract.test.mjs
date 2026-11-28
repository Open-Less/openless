import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
const read = (name) => readFile(new URL(`../src-tauri/src/${name}`, import.meta.url), 'utf8');
const [commands, lib, hotkeys, loops, bridge, host, menu, selection] = await Promise.all([
  read('commands/dictation.rs'), read('lib.rs'), read('hotkey.rs'),
  read('coordinator/hotkey_loops.rs'), read('coordinator/style_menu.rs'),
  read('tauri_coordinator_host.rs'), read('capsule_style_menu.rs'), read('selection.rs'),
]);
const core = await readFile(new URL('../crates/openless-core/src/api.rs', import.meta.url), 'utf8');
const appEvents = await read('tauri_events.rs');
const pickerCommand = await read('commands/capsule_picker.rs');
const windowsFocus = await read('coordinator/style_menu/windows_focus.rs');
assert.match(lib, /commands::get_capsule_style_menu_request/);
assert.match(commands, /pub fn get_capsule_style_menu_request[^]*?capsule_label\(window.label\(\)\)/);
assert.match(commands, /pub async fn set_capsule_style_menu[^]*?request_id: Option<String>,[^]*?menu_id: Option<String>/);
assert.match(commands, /capsule\.set_style_menu_open\(label, session_id, open, request_id, menu_id\)/);
assert.match(commands, /capsule_picker::dispatch\(&core, session_id, open, style,[^]*?capsule\.set_style_menu_open/);
const pickerImpl = commands.slice(commands.indexOf('async fn set_capsule_picker_menu_impl'));
assert.doesNotMatch(pickerImpl, /dictation_style_target|recording_style_required/,
  'command admission must let request-aware style reservation classify stale sessions first');
assert.match(pickerCommand, /if open && !style && !core.dictation_translation_target\(\)/);
assert.match(pickerCommand, /active.mutable && active.session_id == session_id/);
assert.match(pickerCommand, /return Err\("recording_translation_required".into\(\)\)/);
assert.doesNotMatch(pickerCommand, /dictation_style_target|recording_style_required/);
const programmatic = commands.slice(commands.indexOf('pub fn update_dictation_style'), commands.indexOf('pub async fn start_dictation'));
assert.doesNotMatch(programmatic, /hotkey_toggled|handle_skip_auto_style_hotkey/);
assert.equal((loops.match(/handle_skip_auto_style_hotkey\(&inner, /g) ?? []).length, 2,
  'modifier and custom-combination hotkeys share one provenance boundary');
assert.match(bridge, /toggle_dictation_style_with_epoch\(session_id\)[^]*?menu::hotkey_toggled\(&target, epoch\)/);
assert.match(bridge, /DictationPhase::Starting \| openless_core::DictationPhase::Recording/);
assert.match(bridge, /DictationOutputTarget::AskComposer[^]*?DictationOutputTarget::Qa[^]*?DictationOutputTarget::VoiceEdit/);
assert.match(bridge, /mutable: target.mutable && surface_available/);
assert.match(host, /payload.state == CapsuleState::Recording && !payload.qa && !payload.selection_polish/);
assert.match(bridge, /capsule_menu_input_target_is_current\(target\)/);
assert.match(bridge, /capture_capsule_menu_input_target\(\)/);
assert.match(bridge, /#\[cfg\(not\(target_os = "windows"\)\)\]\s*impl FocusState/,
  'Windows must never run the caller-apartment selection/UIA helper');
assert.match(bridge, /windows_focus::Owner::start\(\)/);
assert.match(bridge, /owner.submit\(windows_focus::Scope/);
assert.match(windowsFocus, /name\("capsule-focus-mta".into\(\)\)/);
assert.match(windowsFocus, /CoInitializeEx\(None, COINIT_MULTITHREADED\)/);
const focusRead = windowsFocus.slice(windowsFocus.indexOf('pub(crate) fn current('),
  windowsFocus.indexOf('pub(crate) fn submit('));
assert.doesNotMatch(focusRead, /CoInitialize|GetFocusedElement|CompareElements|recv|sleep|join/,
  'STA getters, reservations, main delivery and hooks read proofs without waiting for UIA');
assert.match(focusRead, /proof.scope != scope/);
assert.match(focusRead, /proof.event != event/);
assert.match(focusRead, /proof.queried.elapsed\(\) > PROOF_LIFETIME/);
assert.match(focusRead, /Native::current\(\) != Some\(proof.native\)/);
assert.match(windowsFocus, /EVENT_SYSTEM_FOREGROUND, EVENT_OBJECT_FOCUS/);
assert.match(windowsFocus, /AddFocusChangedEventHandler/);
assert.match(windowsFocus, /CompareElements\(old, new\)/);
assert.match(windowsFocus, /Native::current\(\) == Some\(request.origin\)/);
assert.match(windowsFocus, /request.queued.elapsed\(\) <= PROOF_LIFETIME/);
assert.match(windowsFocus, /shared\s*\.demand\s*\.lock\(\)\s*\.as_ref\(\)\s*\.is_some_and/);
assert.match(bridge, /configure_focus_fenced/);
assert.match(menu, /focus_before != focus_after/);
assert.match(windowsFocus, /old.queried.elapsed\(\) <= PROOF_LIFETIME/);
assert.match(windowsFocus, /if !identity_same \|\| !continuous/);
assert.match(windowsFocus, /shared.notify\(\)/);
assert.match(windowsFocus, /wanted.epoch > request.scope.epoch/);
assert.doesNotMatch(windowsFocus, /SetFocus\(|SetForegroundWindow\(|SendInput|GetText|CurrentValue/,
  'the observer neither activates a window nor reads or synthesizes text/input');
assert.match(selection, /pub\(crate\) fn capture_capsule_menu_input_target[^]*?target.strict_input_target = true/);
assert.match(menu, /keyboard_available: c.keyboard_available && c.focus.is_some\(\)/);
assert.match(menu, /target.selection != DictationStyleSelection::SkipAuto/);
assert.match(menu, /c\.picker_enabled && c\.skip_auto/);
assert.match(bridge, /dictation_style_target_with_epoch\(\)/);
assert.match(bridge, /configure_fenced/);
assert.match(menu, /c.style_epoch == style_epoch/);
assert.match(menu, /c.style_epoch == active.style_epoch/);
assert.match(menu, /before != after/);
assert.match(menu, /fn drain_key/);
assert.match(core, /fn change_dictation_style[^]*?let mut state = self.state.write\(\)[^]*?previous != selection[^]*?state.advance_dictation_style_epoch\(\)[^]*?Ok\(\(result, state.dictation_style_epoch\)\)/,
  'all style mutation entry points return their own epoch under the mutation lock');
assert.match(core, /fn publish_preferences_changed[^]*?enabled != state.dictation_style_picker_enabled[^]*?state.advance_dictation_style_epoch\(\)/);
assert.match(appEvents, /project_current_style\(&mut snapshot, &current.dictation\)/,
  'late live-session app frames project the current style before presenting a capsule');
assert.match(menu, /label == "capsule"/);
for (const event of ['request', 'key', 'closed']) {
  assert.match(host, new RegExp(`emit_to\\([^;]*"capsule:style-menu-${event}"`));
}
assert.doesNotMatch(host, /\.emit\("capsule:style-menu-/);
assert.match(host, /style_menu_id\.as_deref\(\) == style_menu_id/);
assert.match(host, /let translation_menu = style_menu_id.is_none\(\)/);
assert.match(host, /set_capsule_menu_open\(translation_menu\)/);
assert.match(host, /menu_id\.ok_or\("capsule_style_menu_owner_required"\)/);
const reserve = host.slice(host.indexOf('pub(crate) fn set_style_menu_open'), host.indexOf('fn restore_style_menu_owner'));
assert.ok(reserve.indexOf('reserve_picker_geometry(') < reserve.indexOf('menu::commit('),
  'native geometry must succeed before committing input ownership');
assert.match(host, /payload\.state != CapsuleState::Recording \|\| payload\.selection_polish \|\| !style_session_current/);
assert.match(host, /else \{ payload.state != CapsuleState::Recording \|\| !payload.translation \}/);
assert.match(host, /from_millis\(75\)/, 'foreground/window invalidation must also run without new key input');
assert.match(lib, /WindowEvent::Destroyed[^]*?capsule_style_menu::window_lost\(&label\)/);
const mac = hotkeys.slice(hotkeys.indexOf('extern "C" fn tap_callback'), hotkeys.indexOf('fn device_modifier_pressed'));
assert.ok(mac.indexOf('capsule_style_menu::key(') < mac.indexOf('ask_key_edge('));
assert.match(mac, /FLAG_MASK_SHIFT \| FLAG_MASK_CONTROL[^]*?FLAG_MASK_ALTERNATE \| FLAG_MASK_COMMAND/);
assert.match(mac, /fn_is_held\(\)[^]*?keycode_is_down\(63\)/);
const windows = hotkeys.slice(hotkeys.indexOf('unsafe extern "system" fn low_level_keyboard_proc'), hotkeys.indexOf('unsafe fn callback_context'));
assert.ok(windows.indexOf('capsule_style_menu::key(') < windows.indexOf('match keyboard_edge('));
assert.match(windows, /keyboard.flags.0 & INJECTED == 0/);
assert.match(windows, /GetAsyncKeyState\(vk_code as i32\) < 0/);
assert.match(windows, /match \(vk_code, extended\)/);
assert.match(windows, /\(0x26, true\)/);
assert.match(windows, /\(0x28, true\)/);
assert.match(windows, /\(0x0D, false\)/);
assert.match(hotkeys, /TAP_DISABLED_BY_TIMEOUT \| TAP_DISABLED_BY_USER_INPUT => \{[^}]*crate::capsule_style_menu::invalidate\(\)/);
for (const key of ['VK_LCONTROL', 'VK_RCONTROL', 'VK_LMENU', 'VK_RMENU', 'VK_LWIN', 'VK_RWIN', 'VK_LSHIFT', 'VK_RSHIFT']) {
  assert.ok(windows.includes(key), `Windows modified input must include ${key}`);
}
assert.match(menu, /return press.captured/, 'physical release ownership survives menu lifetime');
assert.match(menu, /Some\(&event.menu_id\) == press.menu_id.as_ref\(\)/,
  'a captured repeat cannot migrate into a new menu owner');
assert.match(menu, /prepared.focus == c.focus && owner.keyboard_available == c.keyboard_available/,
  'geometry completion must recheck the prepared input target and capability');
console.log('capsule-style-menu-host-contract.test.mjs passed');
