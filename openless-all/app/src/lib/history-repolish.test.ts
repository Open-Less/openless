import {
  defaultPackId,
  packDisplayName,
  resolveRepolishRetryPackId,
  resolveRepolishRetryPackIdWithFallback,
} from './history-repolish';
import type { PolishMode, StylePack } from './types';

function assert(condition: boolean, message: string) {
  if (!condition) throw new Error(message);
}

function pack(
  id: string,
  enabled: boolean,
  kind: StylePack['kind'] = 'imported',
  baseMode: PolishMode = 'structured',
): StylePack {
  return {
    id,
    name: `包 ${id}`,
    description: '',
    version: '1.0.0',
    kind,
    baseMode,
    selectionPrompt: '',
    prompt: '',
    examples: [],
    tags: [],
    enabled,
    active: false,
  };
}

const allPacks: StylePack[] = [
  pack('builtin.structured', true),
  pack('custom-alive', true),
  pack('custom-disabled', false),
];

const modeLabel: Record<PolishMode, string> = {
  raw: 'Raw',
  light: 'Light polish',
  structured: 'Structured',
  formal: 'Formal',
};

// 原风格包存在（启用）→ 返回该 id。
assert(
  resolveRepolishRetryPackId({ stylePackId: 'custom-alive' }, allPacks) === 'custom-alive',
  'retry should use the original pack id when the pack still exists',
);

// 原风格包已被禁用 → 仍返回该 id（历史可能出自后来被禁用的包，只要包还在就能重试）。
assert(
  resolveRepolishRetryPackId({ stylePackId: 'custom-disabled' }, allPacks) === 'custom-disabled',
  'retry should use the original pack id even when the pack is disabled',
);

// 内置包同样按原 id 重试。
assert(
  resolveRepolishRetryPackId({ stylePackId: 'builtin.structured' }, allPacks) === 'builtin.structured',
  'retry should use the builtin pack id as-is',
);

// 包已被删除 → 回落（undefined，调用方走当前激活包）。
assert(
  resolveRepolishRetryPackId({ stylePackId: 'deleted-pack' }, allPacks) === undefined,
  'retry should fall back when the original pack was deleted',
);

// 旧历史没有 stylePackId → 回落。
assert(
  resolveRepolishRetryPackId({ stylePackId: null }, allPacks) === undefined,
  'retry should fall back when the record has no stylePackId',
);

// 顶层包列表尚未加载（null）→ 回落。
assert(
  resolveRepolishRetryPackId({ stylePackId: 'custom-alive' }, null) === undefined,
  'retry should fall back while style packs are still loading',
);

// 内置包显示名走 i18n mode 名，自定义包用原名。
assert(
  packDisplayName(pack('builtin.light', true, 'builtin', 'light'), modeLabel) === 'Light polish',
  'builtin packs should display the i18n mode label',
);
assert(
  packDisplayName(pack('custom-alive', true), modeLabel) === '包 custom-alive',
  'custom packs should display their own name',
);

// 下拉默认：当前激活包优先，其次第一个包，空列表为 ''。
assert(
  defaultPackId([
    pack('a', true),
    { ...pack('b', true), active: true },
    pack('c', true),
  ]) === 'b',
  'default should prefer the active pack',
);
assert(
  defaultPackId([pack('a', true), pack('b', true)]) === 'a',
  'default should fall back to the first pack when none is active',
);
assert(defaultPackId([]) === '', 'default should be empty for an empty list');

// 重试回落：原包删除/未加载时显式落到当前激活包（其次第一个），列表全不可用才不传。
const enabledPacks: StylePack[] = [
  { ...pack('active-pack', true), active: true },
  pack('idle-pack', true),
];
assert(
  resolveRepolishRetryPackIdWithFallback({ stylePackId: 'custom-alive' }, allPacks, enabledPacks)
    === 'custom-alive',
  'retry-with-fallback should keep the original pack when it still exists',
);
assert(
  resolveRepolishRetryPackIdWithFallback({ stylePackId: 'deleted-pack' }, allPacks, enabledPacks)
    === 'active-pack',
  'retry-with-fallback should use the active pack when the original was deleted',
);
assert(
  resolveRepolishRetryPackIdWithFallback(
    { stylePackId: null },
    allPacks,
    [pack('only-pack', true)],
  ) === 'only-pack',
  'retry-with-fallback should use the first enabled pack when none is active',
);
assert(
  resolveRepolishRetryPackIdWithFallback({ stylePackId: 'custom-alive' }, null, []) === undefined,
  'retry-with-fallback should stay undefined when no pack list is available',
);
