import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
const read = (name) => readFile(new URL(`../src-tauri/src/${name}`, import.meta.url), 'utf8');
const [commands, lib, hotkeys, loops, bridge, host, menu, selection] = await Promise.all([
  read('commands/dictation.rs'), read('lib.rs'), read('hotkey.rs'),
  read('coordinator/hotkey_loops.rs'), read('coordinator/style_menu.rs'),
  read('tauri_coordinator_host.rs'), read('capsule_style_menu.rs'), read('selection.rs'),
]);
assert.match(lib, /commands::get_capsule_style_menu_request/);
assert.match(commands, /pub fn get_capsule_style_menu_request[^]*?capsule_label\(window.label\(\)\)/);
assert.match(commands, /pub async fn set_capsule_style_menu[^]*?request_id: Option<String>,[^]*?menu_id: Option<String>/);
assert.match(commands, /capsule\.set_style_menu_open\(label, session_id, open, request_id, menu_id\)/);
const programmatic = commands.slice(commands.indexOf('pub fn update_dictation_style'), commands.indexOf('pub async fn start_dictation'));
assert.doesNotMatch(programmatic, /hotkey_toggled|handle_skip_auto_style_hotkey/);
assert.equal((loops.match(/handle_skip_auto_style_hotkey\(&inner, /g) ?? []).length, 2,
  'modifier and custom-combination hotkeys share one provenance boundary');
assert.match(bridge, /toggle_dictation_style_for_session\(session_id\)[^]*?menu::hotkey_toggled\(&target\)/);
assert.match(bridge, /DictationPhase::Starting \| openless_core::DictationPhase::Recording/);
assert.match(bridge, /DictationOutputTarget::AskComposer[^]*?DictationOutputTarget::Qa[^]*?DictationOutputTarget::VoiceEdit/);
assert.match(bridge, /mutable: target.mutable && surface_available/);
assert.match(host, /payload.state == CapsuleState::Recording && !payload.qa && !payload.selection_polish/);
assert.match(bridge, /capsule_menu_input_target_is_current\(target\)/);
assert.match(bridge, /capture_capsule_menu_input_target\(\)/);
assert.match(selection, /pub\(crate\) fn capture_capsule_menu_input_target[^]*?target.strict_input_target = true/);
assert.match(menu, /keyboard_available: c.keyboard_available && c.focus.is_some\(\)/);
assert.match(menu, /target.selection != DictationStyleSelection::SkipAuto/);
assert.match(menu, /c\.picker_enabled && c\.skip_auto/);
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
assert.match(hotkeys, /TAP_DISABLED_BY_TIMEOUT \| TAP_DISABLED_BY_USER_INPUT => \{\s*crate::capsule_style_menu::invalidate\(\)/);
for (const key of ['VK_LCONTROL', 'VK_RCONTROL', 'VK_LMENU', 'VK_RMENU', 'VK_LWIN', 'VK_RWIN', 'VK_LSHIFT', 'VK_RSHIFT']) {
  assert.ok(windows.includes(key), `Windows modified input must include ${key}`);
}
assert.match(menu, /return press.captured/, 'physical release ownership survives menu lifetime');
assert.match(menu, /Some\(&event.menu_id\) == press.menu_id.as_ref\(\)/,
  'a captured repeat cannot migrate into a new menu owner');
assert.match(menu, /prepared.focus == c.focus && owner.keyboard_available == c.keyboard_available/,
  'geometry completion must recheck the prepared input target and capability');
console.log('capsule-style-menu-host-contract.test.mjs passed');
