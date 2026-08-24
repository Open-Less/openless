import { readFile } from 'node:fs/promises';

const sources = Object.fromEntries(
  await Promise.all(
    Object.entries({
      windowsProtocol: '../src-tauri/src/windows_ime_protocol.rs',
      windowsBridge: '../src-tauri/src/windows_ime_ipc.rs',
      windowsSession: '../windows-ime/src/edit_session.cpp',
      windowsPipe: '../windows-ime/src/ipc_client.cpp',
      linuxBridge: '../src-tauri/src/linux_fcitx.rs',
      linuxPlugin: '../../scripts/linux-fcitx5-plugin/openless.cpp',
    }).map(async ([name, relativePath]) => [
      name,
      await readFile(new URL(relativePath, import.meta.url), 'utf8'),
    ]),
  ),
);

function requireTokens(sourceName, tokens) {
  const source = sources[sourceName];
  for (const token of tokens) {
    if (!source.includes(token)) {
      throw new Error(`${sourceName} must contain ${JSON.stringify(token)}`);
    }
  }
}

requireTokens('windowsProtocol', [
  'QueryContext',
  'ContextResult',
  'cursor_utf16',
  'request_id',
]);
requireTokens('windowsBridge', [
  'capture_focused_ime_target',
  'focused_target_block_reason',
  'query_context_over_pipe',
]);
requireTokens('windowsSession', [
  'GetSelection',
  'ShiftStart',
  'ShiftEnd',
  'GUID_PROP_INPUTSCOPE',
  'IS_PASSWORD',
]);
requireTokens('windowsPipe', [
  'queryContext',
  'contextResult',
  'passwordInputScope',
]);

requireTokens('linuxBridge', [
  'GetSurroundingText',
  'read3::<String, u32, String>()',
]);
requireTokens('linuxPlugin', [
  'CapabilityFlag::Password',
  'CapabilityFlag::Sensitive',
  'CapabilityFlag::Terminal',
  'surroundingText()',
  'GetSurroundingText',
  '"sus"',
]);

console.log('context-adapters-contract.test.mjs passed');
