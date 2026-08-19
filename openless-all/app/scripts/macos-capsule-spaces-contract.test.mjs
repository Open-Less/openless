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

// === 卡片借走胶囊窗口后必须完整归还（位置！）契约 ===
//
// 词条卡片和落字回退卡片都不是自己的窗口 —— 它们借用录音胶囊那一个 "capsule"
// 窗口，弹出时把它缩到卡片大小、挪到右下角。收起时必须原样还回去。
//
// 「还位置」这一步曾经漏过一次，真机表现是：用过一次带添加词的卡片之后，
// 下一次录音的胶囊出现在右下角，再也回不到底部居中。漏这一步之所以致命，
// 是因为 maybe_position_capsule_bottom_center 的去重缓存只记「显示器 + 翻译态」，
// 卡片这一挪它一无所知 —— 下次录音拿到相同的显示器快照就判定「没变化」，
// 直接跳过重新定位。窗口被挪走了，而唯一会把它挪回来的那段代码以为自己不用动。
//
// 所以复位和清缓存两件事都要做，各堵一个方向；穿透状态同理（emit_capsule 靠
// capsule_cursor_passthrough 跳过重复调用，缓存与窗口真实状态分家就会跳过
// 该调的那一次，表现是胶囊上的 ✓/✕ 点不动）。
//
// 这段修复在 2026-08 丢过一次（只存在于未合并的本地分支上，主线重新长回了
// 漏位置的版本），靠单测抓不到 —— 它全是 Tauri 窗口调用，跑在 main thread
// 闭包里。契约测试是唯一能钉住它的手段。
const coordinatorRs = (
  await readFile(new URL('../src-tauri/src/coordinator.rs', import.meta.url), 'utf-8')
).replace(/\r\n/g, '\n');

function extractFn(source, name) {
  const match = source.match(new RegExp(`pub\\(crate\\) fn ${name}[\\s\\S]*?\\n}\\n`));
  if (!match) {
    throw new Error(`${name}: function not found in coordinator.rs`);
  }
  return match[0];
}

// 弹卡片 = 把共享窗口挪走，去重缓存必须当场作废。
for (const name of ['show_vocab_suggestion_card', 'show_insert_fallback_card']) {
  const body = extractFn(coordinatorRs, name);
  assertMatch(
    body,
    /\*inner\.capsule_layout\.lock\(\) = None;/,
    `${name} moves the shared capsule window, so it must invalidate the capsule_layout dedup cache`,
  );
  assertMatch(
    body,
    /capsule_cursor_passthrough\s*\.store\(false, Ordering::SeqCst\)/,
    `${name} calls set_ignore_cursor_events directly, so it must keep capsule_cursor_passthrough in sync`,
  );
}

// 收卡片 = 把窗口完整还回去：穿透、尺寸、位置，一样都不能少。
for (const name of ['hide_vocab_suggestion_card', 'hide_insert_fallback_card']) {
  const body = extractFn(coordinatorRs, name);
  assertMatch(
    body,
    /set_ignore_cursor_events\(true\)/,
    `${name} must restore cursor passthrough, or the capsule keeps blocking that strip of screen`,
  );
  assertMatch(
    body,
    /capsule_cursor_passthrough\s*\.store\(true, Ordering::SeqCst\)/,
    `${name} must keep the capsule_cursor_passthrough cache in sync with the window it just touched`,
  );
  assertMatch(
    body,
    /capsule_window_bounds\(false\)[\s\S]*?set_size/,
    `${name} must restore the capsule window size, or the next capsule is squeezed into a card-sized window`,
  );
  assertMatch(
    body,
    /\*inner\.capsule_layout\.lock\(\) = None;/,
    `${name} must invalidate the capsule_layout dedup cache, or the next recording skips repositioning and the capsule stays bottom-right`,
  );
  assertMatch(
    body,
    /position_capsule_bottom_center\(&window, false\)/,
    `${name} must move the capsule window back to bottom-center; restoring size alone leaves it in the card's bottom-right corner`,
  );
  // 顺序不变量：尺寸和位置要一起动，窗口还亮着时改就有概率被合成出一帧
  //「卡片被拉宽、还横着飞过半个屏幕」。
  const hideAt = body.indexOf('window.hide()');
  const resizeAt = body.indexOf('set_size');
  if (hideAt === -1 || hideAt > resizeAt) {
    throw new Error(
      `${name} must hide the window before changing its geometry, or the restore animates on screen`,
    );
  }
}
