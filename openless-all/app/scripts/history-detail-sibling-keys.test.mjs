// Sibling keys inside the history detail panel must be unique.
//
// Background: AudioRecordingPlayer and RepolishPanel are siblings inside the detail Card, and
// both rely on their key to force a remount when the history entry switches, resetting their
// state. Both keys once used plain `item.id`, producing duplicate keys on the same level —
// React only warns ("Encountered two children with the same key") but reconcile can no longer
// match the old fiber, leaving a stale play-recording button in the DOM on every switch; a
// window left open piles up a whole column of them.
//
// This contract pins "sibling keys are distinct at runtime", not any particular naming, so any
// future keyed sibling added to the detail panel is caught too.

import { readFile } from 'node:fs/promises';

const historyTsx = await readFile(new URL('../src/pages/History.tsx', import.meta.url), 'utf-8');

/** Extract the full expression after each `key={` by brace matching (`${}` inside template strings doesn't truncate). */
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

// Detail panel = the right-column Card. Take from the "always rendered on desktop / rendered
// when expanded on mobile" condition to the Card's closing tag.
const detailStart = historyTsx.indexOf('{(!mobile || mobileDetailOpen) && (');
if (detailStart === -1) {
  throw new Error('未定位到历史详情面板（右栏 Card）的起始位置，契约测试需要同步更新');
}
const detailEnd = historyTsx.indexOf('</Card>', detailStart);
if (detailEnd === -1) {
  throw new Error('未定位到历史详情面板的结束位置，契约测试需要同步更新');
}

const detailSource = historyTsx.slice(detailStart, detailEnd);
// List item keys live in the left column, outside this slice; everything here is a key of a
// component at the detail panel's sibling level.
const detailKeys = readKeyExpressions(detailSource);

if (detailKeys.length < 2) {
  throw new Error(
    `历史详情面板里应至少有两个带 key 的组件（播放器与重新润色面板），实际 ${detailKeys.length} 个`,
  );
}

// Each key must still track the entry id, otherwise components don't remount on entry switch
// and the previous entry's playback/polish state bleeds into the next.
for (const { expression } of detailKeys) {
  if (!expression.includes('item.id')) {
    throw new Error(
      `历史详情面板的 key \`${expression}\` 未随条目 id 变化，切换条目时状态会串到下一条`,
    );
  }
}

/**
 * React converts non-undefined keys to strings before sibling reconciliation.
 * Evaluate on a sample entry so different source expressions like `item.id` /
 * `String(item.id)` can't bypass the uniqueness check.
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
        `历史详情面板出现重复的运行时 key：\`${key}\`（表达式 \`${expression}\` 与 ` +
          `\`${seen.get(key)}\` 冲突）。重复 key 会让 React 无法正确删除旧节点，` +
          '切换条目时残留重复的「播放录音」按钮，请给每个组件加上各自的前缀（如 ' +
          '`audio-${item.id}` / `repolish-${item.id}`）。',
      );
    }
    seen.set(key, expression);
  }
}

console.log(`history-detail-sibling-keys: ${detailKeys.length} 个同层 key 均唯一且随条目 id 变化`);
