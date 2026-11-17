import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const appRoot = join(dirname(fileURLToPath(import.meta.url)), '..');
const read = (path) => readFileSync(join(appRoot, path), 'utf8').replace(/\r\n/g, '\n');

// Speech TIPs run alongside the keyboard IME, so this UI must not promise to
// add or remove an entry in Win+Space, regardless of the insertion mode.
const recordingInput = read('src/pages/settings/RecordingInputSection.tsx');
assert.doesNotMatch(recordingInput, /windowsShowOpenlessInKeyboardList/i);
assert.doesNotMatch(recordingInput, /showWindowsOpenlessKeyboardListToggle/);
assert.doesNotMatch(recordingInput, /saveKeyboardListAffectingPrefs/);
assert.doesNotMatch(
  read('src/lib/windowsKeyboardListToggle.ts'),
  /showWindowsOpenlessKeyboardListToggle/,
  'remove the obsolete visibility predicate rather than leaving dead UI policy',
);

const localeFiles = readdirSync(join(appRoot, 'src/i18n')).filter(
  (name) => name.endsWith('.ts') && !name.endsWith('.test.ts') && name !== 'index.ts',
);
assert.ok(localeFiles.includes('en.ts') && localeFiles.includes('zh-CN.ts'));
for (const locale of localeFiles) {
  assert.doesNotMatch(
    read(`src/i18n/${locale}`),
    /windowsShowOpenlessInKeyboardList(?:Label|Desc|Error)/,
    `${locale}: remove the obsolete control label, promise and error message`,
  );
}

// Keep the serialized preference for old settings and cloud-sync documents.
// Removing the UI must not silently remove the wire field or disable speech TSF.
assert.match(read('src/lib/types.ts'), /windowsShowOpenlessInKeyboardList: boolean/);
assert.match(
  read('crates/openless-core/src/shared_types.rs'),
  /rename = "windowsShowOpenlessInKeyboardList"/,
);
const profile = read('src-tauri/src/windows_ime_profile.rs');
const start = profile.indexOf('pub fn apply_windows_openless_keyboard_list(');
const end = profile.indexOf('\n#[cfg(target_os = "windows")]\nmod windows_impl', start);
assert.ok(start >= 0 && end > start, 'locate the profile-application boundary');
const applyProfile = profile.slice(start, end);
assert.match(applyProfile, /set_openless_language_profile_enabled\(true\)/);
assert.doesNotMatch(applyProfile, /set_openless_language_profile_enabled\(desired\)/);

console.log('windows-keyboard-list-removal.test.mjs: ok');
