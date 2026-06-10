import { readFile } from 'node:fs/promises';

function assertMatch(source, pattern, name) {
  if (!pattern.test(source)) {
    throw new Error(`${name}: pattern ${pattern} not found`);
  }
}

const coordinatorRs = (
  await readFile(new URL('../src-tauri/src/coordinator.rs', import.meta.url), 'utf-8')
).replace(/\r\n/g, '\n');
const libRs = (await readFile(new URL('../src-tauri/src/lib.rs', import.meta.url), 'utf-8')).replace(
  /\r\n/g,
  '\n',
);
const functionMatch = coordinatorRs.match(
  /#\[cfg\(target_os = "macos"\)\]\s*fn show_capsule_window_no_activate[\s\S]*?\n}\n\n#\[cfg\(target_os = "linux"\)\]/,
);
const behaviorHelperMatch = coordinatorRs.match(
  /#\[cfg\(target_os = "macos"\)\]\s*fn macos_capsule_collection_behavior[\s\S]*?\n}\n\n#\[cfg\(target_os = "macos"\)\]\s*fn macos_capsule_window_level/,
);
const styleHelperMatch = coordinatorRs.match(
  /#\[cfg\(target_os = "macos"\)\]\s*fn macos_capsule_style_mask[\s\S]*?\n}\n\n#\[cfg\(target_os = "macos"\)\]\s*fn configure_macos_capsule_window_for_overlay/,
);
const hideHelperMatch = coordinatorRs.match(
  /#\[cfg\(target_os = "macos"\)\]\s*pub\(crate\) fn hide_capsule_window_preserving_space[\s\S]*?\n}\n\n#\[cfg\(target_os = "macos"\)\]\s*fn show_capsule_window_no_activate/,
);

if (!functionMatch) {
  throw new Error('macOS capsule no-activate function not found');
}
if (!behaviorHelperMatch) {
  throw new Error('macOS capsule collection behavior helper not found');
}
if (!styleHelperMatch) {
  throw new Error('macOS capsule style helper not found');
}
if (!hideHelperMatch) {
  throw new Error('macOS capsule hide helper not found');
}

const macosNoActivateFunction = functionMatch[0];
const behaviorHelper = behaviorHelperMatch[0];
const styleHelper = styleHelperMatch[0];
const hideHelper = hideHelperMatch[0];
const executableMacosNoActivateFunction = macosNoActivateFunction.replace(/\/\/.*$/gm, '');

assertMatch(
  behaviorHelper,
  /!MOVE_TO_ACTIVE_SPACE[\s\S]*?CAN_JOIN_ALL_SPACES[\s\S]*?TRANSIENT[\s\S]*?IGNORES_CYCLE[\s\S]*?FULL_SCREEN_AUXILIARY/,
  'macOS capsule should clear MoveToActiveSpace and opt into the fullscreen HUD behaviors',
);

assertMatch(
  styleHelper,
  /NONACTIVATING_PANEL[\s\S]*?1 << 7/,
  'macOS capsule should use the non-activating panel style bit',
);

assertMatch(
  macosNoActivateFunction,
  /configure_macos_capsule_window_for_overlay\(window\)[\s\S]*?orderFrontRegardless/,
  'macOS capsule should configure overlay behavior before showing without activation',
);

assertMatch(
  hideHelper,
  /orderOut/,
  'macOS capsule should hide with orderOut to preserve fullscreen Space association',
);

assertMatch(
  libRs,
  /prepare_capsule_window_for_overlay\(&capsule\)[\s\S]*?hide_capsule_window_preserving_space\(&capsule\)/,
  'macOS startup should prepare and hide the capsule through the overlay-preserving path',
);

for (const forbidden of ['set_focus', 'NSApp.activate', 'makeKeyAndOrderFront']) {
  if (executableMacosNoActivateFunction.includes(forbidden)) {
    throw new Error(`macOS capsule no-activate path must not call ${forbidden}`);
  }
}
