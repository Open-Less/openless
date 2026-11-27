import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const read = (path) => readFileSync(new URL(`../${path}`, import.meta.url), 'utf8');

const shortcuts = read('src/features/input/ShortcutsSection.tsx');
const mockSettings = read('src/lib/ipc/mock-data.ts');
assert.match(shortcuts, /supportsSkipAutoStyleHotkey\(platform, capability\?\.adapter\)/);
assert.match(shortcuts, /binding: prefs\.skipAutoStyleHotkey/);
assert.match(
  shortcuts,
  /capability\?\.adapter === 'windowsLowLevel' && binding\.modifiers\.length === 0/,
);
assert.match(shortcuts, /await setSkipAutoStyleHotkey\(binding\)/);
assert.match(shortcuts, /await setSkipAutoStyleHotkey\(null\)/);
assert.match(shortcuts, /on=\{prefs\.skipAutoStylePickerEnabled \?\? false\}/);
assert.match(
  shortcuts,
  /updatePrefs\(\(current\) => \(\{\s*\.\.\.current,\s*skipAutoStylePickerEnabled,?\s*\}\)\)/,
);
assert.match(
  mockSettings,
  /skipAutoStyleHotkey: \{\s*primary: getShortcutPlatform\(\)\.isMac \? 'LeftCommand' : ''/,
);
assert.match(mockSettings, /skipAutoStylePickerEnabled: false/);

const capsule = read('src/features/capsule/Capsule.tsx');
const languageBadge = read('src/features/capsule/CapsuleTranslationBadge.tsx');
const styleBadge = read('src/features/capsule/CapsuleStyleBadge.tsx');
const subscriptions = read('src/api/dictation.ts');
const ipc = read('src/lib/ipc/dictation.ts');
assert.match(capsule, /projectCapsuleOutputStyle\(p\)/);
assert.match(
  capsule,
  /preferences\.showCapsule && preferences\.skipAutoStylePickerEnabled === true/,
);
assert.match(capsule, /<CapsuleStyleBadge/);
for (const badge of [styleBadge, languageBadge]) {
  assert.match(badge, /useCapsulePickerMenu\(\{\s*controller: pickerController\b/);
}
assert.match(styleBadge, /requests\.take\(intent, target\)/);
assert.match(styleBadge, /menu\.owns\(intent\.sessionId, intent\.requestId\)/);
assert.match(styleBadge, /menu\.openMenu\(intent\.requestId\)/);
assert.match(styleBadge, /setCapsuleStyleMenu\(true, sessionId, \{ requestId \}\)/);
assert.match(
  styleBadge,
  /setCapsuleStyleMenu\(false, sessionId, \{ menuId: nativeOwner\.menuId \}\)/,
);
assert.match(styleBadge, /await getCapsuleStyleMenuRequest\(\)/);
assert.match(styleBadge, /setRequest\(\(current\) => forgetStylePickerRequest\(current, stale\)\)/);
assert.match(styleBadge, /setLoaded\(\(current\) => forgetStylePickerRequest\(current, stale\)\)/);
assert.match(
  styleBadge,
  /updateDictationStyle\(intent\.sessionId, \{\s*mode: 'stylePack',\s*stylePackId,?\s*\}\)/,
);
assert.match(styleBadge, /initialFocus=\{false\}\s*finalFocus=\{false\}/);
for (const event of ['request', 'key', 'closed']) {
  assert.ok(
    subscriptions.includes(`events.listen('capsule:style-menu-${event}', onPayload, 'window')`),
  );
}
assert.match(ipc, /invokeOrMock\('get_capsule_style_menu_request', undefined/);
assert.match(
  ipc,
  /'set_capsule_style_menu',\s*\{ open, sessionId, requestId: owner\?\.requestId, menuId: owner\?\.menuId \}/,
);

for (const locale of ['zh-CN', 'zh-TW', 'en', 'de', 'es', 'fr', 'ja', 'ko']) {
  const source = read(`src/i18n/${locale}.ts`);
  for (const key of [
    'skipAutoStyle',
    'skipAutoStylePicker',
    'skipAutoStyleComboRequired',
    'styleChoose',
    'styleChangeFailed',
    'styleStatus',
  ]) {
    assert.match(source, new RegExp(`\\b${key}:\\s*['"][^'"]+`), `${locale} needs ${key}`);
  }
}

console.log(
  'skip-auto-style settings, capsule hooks, picker frontend wire and locale contracts passed',
);
