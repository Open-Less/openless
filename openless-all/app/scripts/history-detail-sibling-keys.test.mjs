// 历史详情面板的同层 key 必须唯一。
//
// 背景：AudioRecordingPlayer 与 RepolishPanel 是详情 Card 里的兄弟节点，两者都靠 key
// 在切换历史条目时强制重挂载以重置自身状态。曾经两个 key 都直接写 `item.id`，同一层出现
// 重复 key —— React 只警告不报错（"Encountered two children with the same key"），
// 但 reconcile 无法正确匹配旧 fiber，每切换一次条目就在 DOM 里残留一个「播放录音」按钮，
// 长时间开着不关的窗口会叠出一整列。
//
// 这条契约锁的是「同层 key 在运行时互不相同」，而不是某个具体命名，后续再往详情面板
// 加带 key 的兄弟组件时同样会被拦下。

import { readFile } from 'node:fs/promises';

const historyTsx = await readFile(
  new URL('../src/pages/History.tsx', import.meta.url),
  'utf-8',
);

/** 从 `key={` 之后开始按花括号配对取出完整表达式（模板串里的 `${}` 不会截断）。 */
function readKeyExpressions(source) {
  const keys = [];
  const marker = 'key={';
  let cursor = 0;
  for (;;) {
    const start = source.indexOf(marker, cursor);
    if (start === -1) return keys;
    let depth = 1;
    let index = start + marker.length;
    while (index < source.length && depth > 0) {
      if (source[index] === '{') depth += 1;
      else if (source[index] === '}') depth -= 1;
      index += 1;
    }
    keys.push({
      expression: source.slice(start + marker.length, index - 1).trim(),
      index: start,
    });
    cursor = index;
  }
}

// 详情面板 = 右栏那张 Card。取「桌面端总是渲染 / 移动端展开才渲染」的条件到 Card 收尾之间。
const detailStart = historyTsx.indexOf('{(!mobile || mobileDetailOpen) && (');
if (detailStart === -1) {
  throw new Error('未定位到历史详情面板（右栏 Card）的起始位置，契约测试需要同步更新');
}
const detailEnd = historyTsx.indexOf('</Card>', detailStart);
if (detailEnd === -1) {
  throw new Error('未定位到历史详情面板的结束位置，契约测试需要同步更新');
}

const detailSource = historyTsx.slice(detailStart, detailEnd);
// 列表项的 key 在左栏，不在这段里；这里拿到的都是详情面板同层组件的 key。
const detailKeys = readKeyExpressions(detailSource);

if (detailKeys.length < 2) {
  throw new Error(
    `历史详情面板里应至少有两个带 key 的组件（播放器与重新润色面板），实际 ${detailKeys.length} 个`,
  );
}

// 每个 key 仍要跟着条目 id 变化，否则切换条目时组件不重挂载，上一条的播放/润色状态会串台。
for (const { expression } of detailKeys) {
  if (!expression.includes('item.id')) {
    throw new Error(
      `历史详情面板的 key \`${expression}\` 未随条目 id 变化，切换条目时状态会串到下一条`,
    );
  }
}

/**
 * React 会把非 undefined 的 key 转成字符串后参与 sibling reconciliation。
 * 在样例条目上求值，避免 `item.id` / `String(item.id)` 这类不同源码表达式绕过唯一性检查。
 */
function evaluateKey(expression, itemId) {
  const value = Function('item', `'use strict'; return (${expression});`)({ id: itemId });
  if (value === undefined) {
    throw new Error(`历史详情面板的 key \`${expression}\` 求值为 undefined`);
  }
  return String(value);
}

for (const itemId of ['history-key-contract-a', 'history-key-contract-b']) {
  const seen = new Map();
  for (const { expression } of detailKeys) {
    const key = evaluateKey(expression, itemId);
    if (seen.has(key)) {
      throw new Error(
        `历史详情面板出现重复的运行时 key：\`${key}\`（表达式 \`${expression}\` 与 `
          + `\`${seen.get(key)}\` 冲突）。重复 key 会让 React 无法正确删除旧节点，`
          + '切换条目时残留重复的「播放录音」按钮，请给每个组件加上各自的前缀（如 '
          + '`audio-${item.id}` / `repolish-${item.id}`）。',
      );
    }
    seen.set(key, expression);
  }
}

console.log(`history-detail-sibling-keys: ${detailKeys.length} 个同层 key 均唯一且随条目 id 变化`);
