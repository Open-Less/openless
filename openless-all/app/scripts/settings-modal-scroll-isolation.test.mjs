import { readFile } from 'node:fs/promises';

function assertEqual(actual, expected, name) {
  if (actual !== expected) {
    throw new Error(`${name}：期望 ${expected}，实际 ${actual}`);
  }
}

function assertMatch(source, pattern, name) {
  if (!pattern.test(source)) {
    throw new Error(`${name}：未找到 ${pattern}`);
  }
}

const floatingShellTsx = await readFile(
  new URL('../src/components/FloatingShell.tsx', import.meta.url),
  'utf-8',
);
const tokensCss = await readFile(
  new URL('../src/styles/tokens.css', import.meta.url),
  'utf-8',
);

const lockAttributeMatches = [
  ...floatingShellTsx.matchAll(/data-ol-settings-open=\{([^}]*)\}/g),
];

assertEqual(
  lockAttributeMatches.length,
  1,
  '背景滚动锁标记应只出现一次',
);

const [lockAttributeMatch] = lockAttributeMatches;
const lockAttributeExpression = lockAttributeMatch[1];
const lockAttributeIndex = lockAttributeMatch.index;
const backgroundScrollerIndex = floatingShellTsx.indexOf('className="ol-thinscroll', lockAttributeIndex);
const settingsModalIndex = floatingShellTsx.indexOf('<SettingsModal', lockAttributeIndex);

assertMatch(
  lockAttributeExpression,
  /\bsettingsOpen\b/,
  '背景滚动锁标记应由设置弹窗状态控制',
);
assertMatch(
  lockAttributeExpression,
  /\bundefined\b/,
  '设置弹窗关闭时应移除背景滚动锁标记',
);
if (!(lockAttributeIndex < backgroundScrollerIndex && backgroundScrollerIndex < settingsModalIndex)) {
  throw new Error('背景滚动锁标记应先于背景滚动区和设置弹窗声明');
}
assertMatch(
  tokensCss,
  /\[data-ol-settings-open\]\s+\.ol-thinscroll\s*\{[^}]*overflow:\s*hidden\s*!important\s*;?[^}]*\}/s,
  '背景层内的所有细滚动区应在设置弹窗打开时停止滚动',
);
assertMatch(
  tokensCss,
  /\[data-ol-settings-open\]\s+\.ol-thinscroll::\-webkit-scrollbar\s*\{[^}]*display:\s*none\s*;?[^}]*\}/s,
  'WebKitGTK 下应直接隐藏背景细滚动条',
);
