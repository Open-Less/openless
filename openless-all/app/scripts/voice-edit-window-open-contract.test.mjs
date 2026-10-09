import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const appRoot = fileURLToPath(new URL('..', import.meta.url));
const read = (relativePath) => readFile(join(appRoot, relativePath), 'utf8');

const commands = await read('src-tauri/src/commands/voice_edit.rs');
const openStart = commands.indexOf('pub async fn voice_edit_window_open');
const openEnd = commands.indexOf('#[tauri::command]', openStart);
const openCommand = commands.slice(openStart, openEnd);
assert(openStart !== -1, 'Voice Edit must open through an async Tauri command');
assert(
  openCommand.includes('window.label() != "main"') &&
    openCommand.indexOf('window.label() != "main"') < openCommand.indexOf('spawn_blocking'),
  'Voice Edit open stays restricted to the main window before creating a WebView',
);
assert(
  openCommand.includes('tauri::async_runtime::spawn_blocking(move || host.show_voice_edit())') &&
    /\.await\s*\n\s*\.map_err\(/.test(openCommand),
  'Windows WebView creation must leave the IPC thread and return worker errors',
);

const host = await read('src-tauri/src/tauri_coordinator_host.rs');
const showStart = host.indexOf('pub(crate) fn show_voice_edit(');
const showEnd = host.indexOf('pub(crate) fn ', showStart + 1);
const show = host.slice(showStart, showEnd);
assert(
  show.includes('Result<(), String>') && show.includes('crate::show_voice_edit_window(&app)'),
  'Voice Edit window creation failures must propagate to the caller',
);

const settings = await read('src/pages/settings/SelectionWorkspaceSection.tsx');
assert(
  settings.includes('openPanelBusy') &&
    settings.includes('aria-busy={openPanelBusy}') &&
    settings.includes("t('common.operationFailed')") &&
    settings.includes('setOpenPanelError'),
  'the Voice Edit open button must show a busy state and surface window errors',
);

console.log('voice-edit-window-open-contract.test.mjs passed');
