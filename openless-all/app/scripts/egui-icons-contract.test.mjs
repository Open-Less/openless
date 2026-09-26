import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { icons } from './sync-egui-lucide-icons.mjs';

const app = resolve(fileURLToPath(new URL('..', import.meta.url)));
const linux = readFileSync(resolve(app, 'linux-egui/src/ui/frontend/icons.rs'), 'utf8');
const tauri = readFileSync(resolve(app, 'src/components/Icon.tsx'), 'utf8');
const nav = readFileSync(resolve(app, 'linux-egui/src/ui/frontend/layout.rs'), 'utf8');
const settings = readFileSync(resolve(app, 'linux-egui/src/ui/frontend/settings.rs'), 'utf8');
const tauriSettings = readFileSync(resolve(app, 'src/pages/settings/navigation.ts'), 'utf8');
const tauriNav = readFileSync(resolve(app, 'src/components/FloatingShell.tsx'), 'utf8');
execFileSync(process.execPath, [resolve(app, 'scripts/sync-egui-lucide-icons.mjs'), '--check']);
for (const [name, glyph] of Object.entries(icons)) {
  const start = linux.indexOf(`IconName::${name} =>`);
  assert.ok(start >= 0, `${name} has no Rust match arm`);
  const arm = linux.slice(start, linux.indexOf('IconName::', start + 1));
  assert.ok(arm.includes(`"${name}"`) && arm.includes(`../../../assets/lucide/${name}.svg`), `${name} is not wired to its SVG`);
  const key = {
    Overview: 'overview', History: 'history', Vocab: 'vocab', Style: 'style',
    SelectionAsk: 'selectionAsk', Settings: 'settings', Mic: 'mic',
    Sparkle: 'sparkle', Hash: 'hash', Clock: 'clock', Bolt: 'bolt', Copy: 'copy',
    Search: 'search', Trash: 'trash', Refresh: 'refresh', Download: 'download',
    Upload: 'upload', Plus: 'plus', Play: 'play', Close: 'close', Check: 'check',
    More: 'more', ChevronRight: 'chevRight', Feather: 'feather', Layout: 'layout',
    Doc: 'doc', Pencil: 'pencil', Cloud: 'cloud', Shield: 'shield',
    Info: 'info', Help: 'help', External: 'external', Monitor: 'mac',
  }[name];
  if (key) assert.match(tauri, new RegExp(`\\b${key}: ${glyph}\\b`), `${name} differs from Tauri's icon mapping`);
}
for (const [section, icon] of Object.entries({
  General: 'Mic', Shortcuts: 'Bolt', Services: 'Cloud', Appearance: 'Settings',
  Privacy: 'Shield', Advanced: 'Sparkle', About: 'Info',
})) {
  assert.ok(settings.includes(`Self::${section} => SettingsIcon::${icon}`), `${section} settings rail icon differs from Tauri`);
  assert.ok(settings.includes(`SettingsIcon::${icon} => IconName::${icon}`), `${icon} must use the Lucide SVG`);
  assert.match(tauriSettings, new RegExp(`id: '${section.toLowerCase()}', icon: '${icon.toLowerCase()}'`));
}
const tools = nav.slice(nav.indexOf('"nav.group_tools"'), nav.indexOf('"nav.settings"'));
const expected = ['nav.translation', 'nav.selection_ask', 'nav.quickNote', 'nav.corrections'];
let previous = -1;
for (const item of expected) {
  const at = tools.indexOf(`"${item}"`);
  assert.ok(at > previous, `${item} must follow the previous tools entry`);
  previous = at;
}
assert.equal(nav.slice(nav.indexOf('"nav.history"'), nav.indexOf('"nav.group_tools"')).includes('"nav.quickNote"'), false);
assert.match(tauriNav, /\{ id: 'selectionAsk' \},\s*\{ id: 'quickNote' \},\s*\{ id: 'corrections' \}/);
console.log('egui SVG outlines and tools navigation match Tauri');
