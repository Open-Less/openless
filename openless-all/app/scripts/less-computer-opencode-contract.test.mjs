import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const appRoot = fileURLToPath(new URL('..', import.meta.url));
const read = (relativePath) => readFile(join(appRoot, relativePath), 'utf8');

const [settings, ipc, opencode, dictation, coordinator, lib, onboarding, lessComputerIpc, qaCommands, credentialCommands, miscCommands] = await Promise.all([
  read('src/pages/settings/CodingAgentSection.tsx'),
  read('src/lib/ipc/coding-agent.ts'),
  read('src-tauri/src/coding_agent/opencode.rs'),
  read('src-tauri/src/coordinator/dictation.rs'),
  read('src-tauri/src/coordinator.rs'),
  read('src-tauri/src/lib.rs'),
  read('src/components/Onboarding.tsx'),
  read('src/lib/ipc/less-computer.ts'),
  read('src-tauri/src/commands/qa.rs'),
  read('src-tauri/src/commands/credentials.rs'),
  read('src-tauri/src/commands/misc.rs'),
]);

assert(
  settings.includes('codingAgentListOpencodeModels(exe, true)'),
  'OpenCode settings must automatically refresh the available models after detection',
);
assert(
  settings.includes('settings.codingAgent.opencodeModelsRefresh'),
  'OpenCode settings must expose the localized manual refresh action',
);
assert(
  ipc.includes('"coding_agent_list_opencode_models"'),
  'frontend IPC must call the OpenCode model-list command',
);
assert(opencode.includes('"--refresh"'), 'OpenCode model discovery must refresh its model cache');
assert(opencode.includes('"--auto"'), 'OpenCode runs must use the current automatic permission flag');
assert(opencode.includes('"--continue"'), 'OpenCode runs must preserve follow-up session context');
assert(
  !opencode.includes('--dangerously-skip-permissions'),
  'OpenCode adapter must not use the removed Claude-style permission flag',
);
assert(
  dictation.includes('resolve_coding_agent_model(provider'),
  'Less Computer must resolve model defaults per provider',
);

const microphoneMenuStart = lib.indexOf('fn build_microphone_tray_menu');
const microphoneMenuEnd = lib.indexOf('pub(crate) fn refresh_tray_microphone_menu');
const microphoneMenu = lib.slice(microphoneMenuStart, microphoneMenuEnd);
assert(
  microphoneMenu.includes('TrayMicrophoneDeviceCache'),
  'tray menu construction must read the asynchronous microphone-device cache',
);
assert(
  !microphoneMenu.includes('recorder::list_input_devices'),
  'tray menu construction must never enumerate CoreAudio devices on the AppKit main thread',
);
assert(
  lib.includes('.name("openless-tray-mic-event".into())'),
  'native microphone notifications must dispatch enumeration to a background thread',
);
assert(
  credentialCommands.includes('pub async fn get_credentials()') &&
    credentialCommands.includes('tauri::async_runtime::spawn_blocking'),
  'Keychain reads must not block the AppKit main thread while settings load',
);
assert(
  miscCommands.includes('pub async fn list_microphone_devices()') &&
    miscCommands.includes('microphone device worker failed'),
  'settings microphone enumeration must not block the AppKit main thread',
);
assert(
  onboarding.includes("t('onboarding.continueToSettings')"),
  'users must be able to configure OpenCode without granting unrelated system permissions first',
);
assert(
  settings.includes('lessComputerWindowOpen()') &&
    lessComputerIpc.includes('"less_computer_window_open"') &&
    qaCommands.includes('window.label() != "main"'),
  'Advanced settings must expose a main-window-only text entry point for Less Computer',
);
const showLessComputerStart = lib.indexOf('pub(crate) fn show_less_computer_window');
const showLessComputerEnd = lib.indexOf('pub(crate) fn hide_less_computer_window');
const showLessComputer = lib.slice(showLessComputerStart, showLessComputerEnd);
assert(
  showLessComputer.includes('window_clone.show()'),
  'the first lazy Less Computer window must clear Tauri visible=false before ordering its NSPanel',
);
assert(
  showLessComputer.indexOf('run_on_main_thread') <
    showLessComputer.indexOf('position_less_computer_window(&window_clone)'),
  'Less Computer NSPanel positioning must run on the AppKit main thread',
);
const submitTextStart = coordinator.indexOf('pub fn less_computer_submit_text');
const submitTextEnd = coordinator.indexOf('pub fn history', submitTextStart);
const submitText = coordinator.slice(submitTextStart, submitTextEnd);
assert(
  submitText.includes('tauri::async_runtime::spawn') && !submitText.includes('tokio::spawn'),
  'Less Computer text submit must spawn through the Tauri runtime from the WebKit IPC thread',
);

const localeFiles = ['zh-CN.ts', 'zh-TW.ts', 'en.ts', 'ja.ts', 'ko.ts'];
const localizedKeys = [
  'opencodeModelDefault',
  'opencodeModelHint',
  'opencodeModelsRefresh',
  'opencodeModelsRefreshing',
  'opencodeModelsLoaded',
  'opencodeModelsEmpty',
  'opencodeModelsError',
  'openPanel',
  'openPanelHint',
  'openPanelAction',
];
for (const localeFile of localeFiles) {
  const locale = await read(`src/i18n/${localeFile}`);
  assert(locale.includes('continueToSettings:'), `${localeFile} is missing continueToSettings`);
  for (const key of localizedKeys) {
    assert(locale.includes(`${key}:`), `${localeFile} is missing ${key}`);
  }
}

console.log('less-computer-opencode-contract.test.mjs passed');
