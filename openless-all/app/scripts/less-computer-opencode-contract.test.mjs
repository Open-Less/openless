import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const appRoot = fileURLToPath(new URL('..', import.meta.url));
const read = (relativePath) => readFile(join(appRoot, relativePath), 'utf8');

const shortcuts = await read('src/pages/settings/ShortcutsSection.tsx');
const agentSettings = await read('src/pages/settings/CodingAgentSection.tsx');
const settingsTabs = await read('src/pages/settings/tabs.tsx');
assert(
  !/if \(os === 'win'/.test(agentSettings),
  'Windows must expose Less Computer configuration and its text entry point',
);
assert(
  shortcuts.includes("(os === 'mac' || os === 'win') && ("),
  'Windows must expose the Less Computer voice shortcut',
);
// Platform visibility is covered by navigation.test.ts; the detail route must
// still mount the existing settings consumer.
assert(
  settingsTabs.includes("item.id === 'lessComputer' && <CodingAgentSection />"),
  'the Less Computer subpage must mount its settings consumer',
);

const [
  settings,
  ipc,
  opencode,
  dictation,
  lib,
  onboarding,
  lessComputerIpc,
  qaCommands,
  credentialCommands,
  miscCommands,
  coreAdapters,
  coordinatorHost,
  coreApi,
] = await Promise.all([
  read('src/pages/settings/CodingAgentSection.tsx'),
  read('src/lib/ipc/coding-agent.ts'),
  read('crates/openless-core/src/coding_agent.rs'),
  read('crates/openless-core/src/api.rs'),
  read('src-tauri/src/lib.rs'),
  read('src/components/Onboarding.tsx'),
  read('src/lib/ipc/less-computer.ts'),
  read('src-tauri/src/commands/qa.rs'),
  read('src-tauri/src/commands/credentials.rs'),
  read('src-tauri/src/commands/misc.rs'),
  read('src-tauri/src/core_adapters.rs'),
  read('src-tauri/src/tauri_coordinator_host.rs'),
  read('crates/openless-core/src/api.rs'),
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
  /['"]coding_agent_list_opencode_models['"]/.test(ipc),
  'frontend IPC must call the OpenCode model-list command',
);
assert(opencode.includes('"--refresh"'), 'OpenCode model discovery must refresh its model cache');
assert(
  opencode.includes('"--auto"'),
  'OpenCode runs must use the current automatic permission flag',
);
assert(opencode.includes('"--continue"'), 'OpenCode runs must preserve follow-up session context');
assert(
  !opencode.includes('--dangerously-skip-permissions'),
  'OpenCode adapter must not use the removed Claude-style permission flag',
);
assert(
  coreApi.includes('resolve_coding_agent_model(provider') &&
    dictation.includes('submit_less_computer_with_session'),
  'Less Computer must resolve model defaults per provider',
);
assert(
  settings.includes(
    "const SANDBOX_PERMISSION_MODES: CodingAgentPermissionMode[] = ['plan', 'acceptEdits']",
  ) &&
    settings.includes("provider === 'codex-cli' || provider === 'dsh-cli'") &&
    settings.includes('normalizePermissionMode'),
  'Codex and dsh settings must expose only read-only/plan and workspace-write permission modes and normalize legacy values',
);
const providerChangeStart = settings.indexOf('const nextProvider = v as CodingAgentProviderId');
const providerChangeEnd = settings.indexOf('options={PROVIDERS}', providerChangeStart);
const providerChange = settings.slice(providerChangeStart, providerChangeEnd);
assert(
  providerChange.includes('codingAgentModel: null') &&
    providerChange.includes('codingAgentExe: null') &&
    providerChange.includes('normalizePermissionMode'),
  'switching coding-agent provider must clear provider-specific model/executable state and normalize permissions',
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
  credentialCommands.includes('pub async fn get_credentials(') &&
    credentialCommands.includes('core.get_credentials_status()') &&
    credentialCommands.includes('tauri::async_runtime::spawn_blocking'),
  'Keychain reads must not block the AppKit main thread while settings load',
);
assert(
  miscCommands.includes('pub async fn list_microphone_devices(') &&
    miscCommands.includes('.platform') &&
    miscCommands.includes('.microphone_devices()') &&
    coreAdapters.includes(
      'tauri::async_runtime::spawn_blocking(crate::recorder::list_input_devices',
    ),
  'settings microphone enumeration must not block the AppKit main thread',
);
assert(
  onboarding.includes("t('onboarding.continueToSettings')"),
  'users must be able to configure OpenCode without granting unrelated system permissions first',
);
assert(
  settings.includes('lessComputerWindowOpen()') &&
    /['"]less_computer_window_open['"]/.test(lessComputerIpc) &&
    qaCommands.includes('window.label() != "main"'),
  'Advanced settings must expose a main-window-only text entry point for Less Computer',
);
const openWindowStart = qaCommands.indexOf('pub async fn less_computer_window_open');
assert(openWindowStart !== -1, 'Less Computer must open through an async Tauri command');
const openWindowEnd = qaCommands.indexOf('/// Pulls the current session', openWindowStart);
const openWindow = qaCommands.slice(openWindowStart, openWindowEnd);
assert(
  openWindow.includes('tauri::async_runtime::spawn_blocking(move || host.show_less_computer())') &&
    /\.await\s*\.map_err\([^;]+\)\?/.test(openWindow) &&
    openWindow.indexOf('window.label() != "main"') < openWindow.indexOf('spawn_blocking') &&
    !openWindow.includes('Ok(())'),
  'Windows window creation must leave the IPC thread and propagate worker and window errors',
);
const hostShowStart = coordinatorHost.indexOf('pub(crate) fn show_less_computer(');
const hostShowEnd = coordinatorHost.indexOf('pub(crate) fn ', hostShowStart + 1);
const hostShow = coordinatorHost.slice(hostShowStart, hostShowEnd);
assert(
  hostShow.includes('Result<(), String>') &&
    hostShow.includes('.ok_or_else(') &&
    hostShow.includes('crate::show_less_computer_window(&app)'),
  'Less Computer host must propagate an unbound AppHandle or window failure',
);
assert(
  /HostAction::ShowLessComputer\s*=>\s*\{?\s*crate::show_less_computer_window\(&app\)\s*\.map_err\(map_tauri_error\)\?/.test(
    coreAdapters,
  ),
  'Core host actions must propagate Less Computer window errors as platform errors',
);
const openPanelStart = settings.indexOf('const openPanel = async () =>');
const openPanelEnd = settings.indexOf('\n  };', openPanelStart);
const openPanel = settings.slice(openPanelStart, openPanelEnd);
assert(
  openPanelStart !== -1 &&
    openPanel.includes('await lessComputerWindowOpen()') &&
    openPanel.includes('error instanceof Error ? error.message : String(error)') &&
    /finally\s*\{\s*setOpenPanelBusy\(false\)/.test(openPanel) &&
    settings.includes('disabled={openPanelBusy}') &&
    settings.includes('aria-busy={openPanelBusy}') &&
    settings.includes('role="alert"') &&
    settings.includes("t('common.operationFailed')"),
  'Less Computer settings must show opening and failure states and restore the button for retry',
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
const submitTextStart = qaCommands.indexOf('pub fn less_computer_submit_text');
const windowsShowStart = showLessComputer.indexOf('#[cfg(target_os = "windows")]');
const windowsShowEnd = showLessComputer.indexOf('#[cfg(not(', windowsShowStart);
const windowsShow = showLessComputer.slice(windowsShowStart, windowsShowEnd);
assert(
  windowsShow.includes('Result<(), String>') &&
    windowsShow.includes('ensure_less_computer_window(app)?') &&
    /window\.show\(\)\.map_err\([\s\S]+?\)\?;/.test(windowsShow) &&
    windowsShow.indexOf('window.show()') < windowsShow.indexOf('Ok(())'),
  'Windows Less Computer must return creation/show failures before reporting success',
);
const submitTextEnd = qaCommands.indexOf(
  '/// Text testing entry from the main settings page',
  submitTextStart,
);
const submitText = qaCommands.slice(submitTextStart, submitTextEnd);
assert(
  submitText.includes('host.spawn') &&
    submitText.includes('backend.submit_less_computer(text)') &&
    !submitText.includes('tauri::async_runtime::spawn') &&
    !submitText.includes('tokio::spawn') &&
    coordinatorHost.includes('tauri::async_runtime::spawn(future)'),
  'Less Computer text submit must spawn through the explicit Tauri host from the WebKit IPC thread',
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
  'codexBudgetHint',
  'codexMode',
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
