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
    voiceEditPrompt: '',
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

// Original pack exists (enabled) → returns that id.
assert(
  resolveRepolishRetryPackId({ stylePackId: 'custom-alive' }, allPacks) === 'custom-alive',
  'retry should use the original pack id when the pack still exists',
);

// Original pack is disabled → still returns that id (history may come from a since-disabled pack; retry works while the pack exists).
assert(
  resolveRepolishRetryPackId({ stylePackId: 'custom-disabled' }, allPacks) === 'custom-disabled',
  'retry should use the original pack id even when the pack is disabled',
);

// Builtin packs retry by their original id too.
assert(
  resolveRepolishRetryPackId({ stylePackId: 'builtin.structured' }, allPacks) ===
    'builtin.structured',
  'retry should use the builtin pack id as-is',
);

// Pack was deleted → fall back (undefined; caller uses the currently active pack).
assert(
  resolveRepolishRetryPackId({ stylePackId: 'deleted-pack' }, allPacks) === undefined,
  'retry should fall back when the original pack was deleted',
);

// Older history has no stylePackId → fall back.
assert(
  resolveRepolishRetryPackId({ stylePackId: null }, allPacks) === undefined,
  'retry should fall back when the record has no stylePackId',
);

// Pack list not yet loaded (null) → fall back.
assert(
  resolveRepolishRetryPackId({ stylePackId: 'custom-alive' }, null) === undefined,
  'retry should fall back while style packs are still loading',
);

// Builtin pack display name uses the i18n mode label; custom packs keep their own name.
assert(
  packDisplayName(
    { ...pack('builtin.light', true, 'builtin', 'light'), name: '轻度润色' },
    modeLabel,
  ) === 'Light polish',
  'builtin packs should display the i18n mode label',
);
assert(
  packDisplayName(pack('builtin.light', true, 'builtin', 'light'), modeLabel) ===
    '包 builtin.light',
  'user-renamed builtin packs retain their chosen name',
);
assert(
  packDisplayName(pack('custom-alive', true), modeLabel) === '包 custom-alive',
  'custom packs should display their own name',
);

// Dropdown default: prefer the active pack, then the first pack, empty list yields ''.
assert(
  defaultPackId([pack('a', true), { ...pack('b', true), active: true }, pack('c', true)]) === 'b',
  'default should prefer the active pack',
);
assert(
  defaultPackId([pack('a', true), pack('b', true)]) === 'a',
  'default should fall back to the first pack when none is active',
);
assert(defaultPackId([]) === '', 'default should be empty for an empty list');

// Retry fallback: when the original pack was deleted / not loaded, explicitly fall to the currently active
// pack (then the first one); pass nothing only when the whole list is unusable.
const enabledPacks: StylePack[] = [
  { ...pack('active-pack', true), active: true },
  pack('idle-pack', true),
];
assert(
  resolveRepolishRetryPackIdWithFallback(
    { stylePackId: 'custom-alive' },
    allPacks,
    enabledPacks,
  ) === 'custom-alive',
  'retry-with-fallback should keep the original pack when it still exists',
);
assert(
  resolveRepolishRetryPackIdWithFallback(
    { stylePackId: 'deleted-pack' },
    allPacks,
    enabledPacks,
  ) === 'active-pack',
  'retry-with-fallback should use the active pack when the original was deleted',
);
assert(
  resolveRepolishRetryPackIdWithFallback({ stylePackId: null }, allPacks, [
    pack('only-pack', true),
  ]) === 'only-pack',
  'retry-with-fallback should use the first enabled pack when none is active',
);
assert(
  resolveRepolishRetryPackIdWithFallback({ stylePackId: 'custom-alive' }, null, []) === undefined,
  'retry-with-fallback should stay undefined when no pack list is available',
);
