import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const read = (path) => readFile(new URL(`../${path}`, import.meta.url), 'utf8');

const [pinModule, coreRemote, coreContract, tauriAdapter, coordinator, command, appCargo, ci] =
  await Promise.all([
    read('src-tauri/src/remote_server/pin_persistence.rs'),
    read('crates/openless-core/src/remote_input_service.rs'),
    read('crates/openless-core/tests/remote_input_contract.rs'),
    read('src-tauri/src/core_adapters.rs'),
    read('src-tauri/src/coordinator.rs'),
    read('src-tauri/src/commands/remote_input.rs'),
    read('src-tauri/Cargo.toml'),
    read('../../.github/workflows/ci.yml'),
  ]);

for (const token of ['O_NOFOLLOW', 'O_NONBLOCK', 'O_CLOEXEC']) {
  assert.match(
    pinModule,
    new RegExp(`custom_flags\\([^)]*${token}`),
    `Unix open must use ${token}`,
  );
}
assert.match(pinModule, /file\.metadata\(\)/, 'validation must fstat the opened file');
assert.match(pinModule, /file\.set_permissions\(/, 'permission repair must use the opened file');
assert.match(
  pinModule,
  /\.take\(MAX_PIN_FILE_BYTES \+ 1\)/,
  'reads must remain bounded after fstat',
);

for (const token of [
  'FILE_FLAG_OPEN_REPARSE_POINT',
  'GetFileInformationByHandle',
  'GetFileType',
  'FILE_ATTRIBUTE_REPARSE_POINT',
  'nNumberOfLinks',
  'ReplaceFileW',
  'MoveFileExW',
]) {
  assert.match(pinModule, new RegExp(token), `Windows implementation must use ${token}`);
}
assert.doesNotMatch(
  pinModule,
  /remove_file\(path\)/,
  'replacement must never delete the destination before installing the new PIN',
);
assert.match(pinModule, /backup_path/, 'Windows replacement must retain a rollback path');

assert.match(appCargo, /"Win32_Storage_FileSystem"/, 'Windows file APIs must be enabled');
assert.match(
  pinModule,
  /#\[cfg\(test\)\][\s\S]*mod tests[\s\S]*hard_link_pin_path_is_rejected/,
  'PIN persistence security tests must remain in the owning Tauri module',
);
assert.match(
  ci,
  /if: runner\.os != 'Windows'[\s\S]*cargo test --locked --manifest-path src-tauri\/Cargo\.toml --lib/,
  'non-Windows CI must execute the owning Tauri module tests',
);
assert.match(
  ci,
  // `--bins` was folded in when the duplicate `cargo check` pass was removed;
  // the requirement is only that Windows compiles the owning module's tests.
  /if: runner\.os == 'Windows'[\s\S]*cargo test --locked --manifest-path src-tauri\/Cargo\.toml --lib(?: --bins)? --no-run/,
  'Windows CI must at least compile the owning Tauri module tests when native runtime DLLs prevent execution',
);

// The legacy file helper remains hardened for older host adapters. The current
// Core keeps short-lived admission PINs in memory and must never await this I/O.
assert.doesNotMatch(
  coreRemote,
  /self\.runtime\.(?:load_pairing_pin|persist_pairing_pin)\(/,
  'ephemeral PIN rotation must not depend on legacy disk storage',
);
assert.match(
  coreContract,
  /rotation_does_not_read_or_write_legacy_pin_storage/,
  'Core must exercise rotation even when legacy persistence rejects writes',
);
assert.match(
  tauriAdapter,
  /impl openless_core::RemoteInputRuntimeAdapter for TauriRemoteInputRuntimeAdapter[\s\S]*?fn persist_pairing_pin[\s\S]*?crate::remote_server::save_pin/,
  'Tauri adapter must delegate pairing PIN persistence to the hardened atomic file implementation',
);
assert.doesNotMatch(
  coordinator,
  /regenerate_remote_pin|persist_and_commit_remote_pin|remote_server_handle|pairing_pin/,
  'Coordinator must not regain remote input PIN or transport ownership',
);
assert.match(
  command,
  /pub async fn regenerate_remote_pin[\s\S]*?-> Result<String, String>[\s\S]*?regenerate_pairing_pin\(\)[\s\S]*?\.await[\s\S]*?read_pairing_pin\(\)/,
  'Tauri command must surface Core reset failures before returning the committed PIN',
);

console.log('PIN persistence security contract passed');
