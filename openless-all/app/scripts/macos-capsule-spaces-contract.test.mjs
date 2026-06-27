import { readFile } from 'node:fs/promises';

function assertMatch(source, pattern, name) {
  if (!pattern.test(source)) {
    throw new Error(`${name}: pattern ${pattern} not found`);
  }
}

// 契约函数 show_capsule_window_no_activate 的实现现位于编译进二进制的
// coordinator/capsule_focus.rs（2026-06 板块化重构从 coordinator.rs 迁出，行为不变；
// 函数可见性随迁移改为 pub(super)）。契约必须校验真正编译的那份，否则会出现
// 「测试绿、线上坏」的假信心。
const capsuleFocusRs = (
  await readFile(new URL('../src-tauri/src/coordinator/capsule_focus.rs', import.meta.url), 'utf-8')
).replace(/\r\n/g, '\n');
const functionMatch = capsuleFocusRs.match(
  /#\[cfg\(target_os = "macos"\)\]\s*(?:pub\((?:crate|super)\) )?fn show_capsule_window_no_activate[\s\S]*?\n}\n\n#\[cfg\(target_os = "linux"\)\]/,
);

if (!functionMatch) {
  throw new Error('macOS capsule no-activate function not found');
}

const macosNoActivateFunction = functionMatch[0];
const executableMacosNoActivateFunction = macosNoActivateFunction.replace(/\/\/.*$/gm, '');

assertMatch(
  macosNoActivateFunction,
  /CAN_JOIN_ALL_SPACES[\s\S]*?1 << 0[\s\S]*?setCollectionBehavior[\s\S]*?orderFrontRegardless/,
  'macOS capsule should join all Spaces via an absolute collectionBehavior write before showing without activation',
);

assertMatch(
  macosNoActivateFunction,
  /FULL_SCREEN_AUXILIARY[\s\S]*?1 << 8[\s\S]*?setCollectionBehavior[\s\S]*?orderFrontRegardless/,
  'macOS capsule should join fullscreen Spaces as an auxiliary window before showing without activation',
);

assertMatch(
  macosNoActivateFunction,
  /setLevel:\s*25[\s\S]*?orderFrontRegardless/,
  'macOS capsule must raise window level above the menu bar (25) so it renders over fullscreen apps, not just behind them',
);

for (const forbidden of ['window.show()', 'set_focus', 'NSApp.activate', 'makeKeyAndOrderFront']) {
  if (executableMacosNoActivateFunction.includes(forbidden)) {
    throw new Error(`macOS capsule no-activate path must not call ${forbidden}`);
  }
}

// === 胶囊跟随「鼠标光标所在屏」契约（多屏 / 多 Space）===
// 根因：定位用 AX caret、layout 去重缓存却用胶囊自己的 current_monitor，两者看
// 不同的屏；光标移到另一块屏时缓存误判「没变化」→ 跳过重新定位 → 胶囊被锁死
// 在第一块屏（别屏只闪一下）。修复后两条路径必须共用 capsule_target_monitor，
// 且以鼠标光标为首选信号。这些不变量纯靠源码 grep 守护，无法在无多屏硬件的
// 单测里覆盖，正是契约测试的用武之地。
const libRs = (
  await readFile(new URL('../src-tauri/src/lib.rs', import.meta.url), 'utf-8')
).replace(/\r\n/g, '\n');

assertMatch(
  libRs,
  /fn capsule_target_monitor[\s\S]*?macos_mouse_cursor_point\(\)\s*\.or_else\(\s*macos_focused_input_anchor_point\s*\)/,
  'macOS capsule must resolve its target monitor from the mouse cursor first, AX caret only as fallback',
);

assertMatch(
  libRs,
  /跟随鼠标光标所在显示器[\s\S]*?if let Some\(mon\) = capsule_target_monitor\(window\)/,
  'macOS capsule positioning must follow capsule_target_monitor (the mouse screen), not its own current_monitor',
);

assertMatch(
  capsuleFocusRs,
  /#\[cfg\(target_os = "macos"\)\][\s\S]*?crate::capsule_target_monitor\(window\)/,
  'macOS capsule layout cache key must reuse capsule_target_monitor, or it will skip repositioning when the cursor moves to another screen',
);
