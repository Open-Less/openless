import assert from "node:assert/strict"
import { readFile } from "node:fs/promises"

const read = (path) => readFile(new URL(`../${path}`, import.meta.url), "utf8")

const [pinModule, coreRemote, coreContract, tauriAdapter, coordinator, command, appCargo, ci] =
    await Promise.all([
        read("src-tauri/src/remote_server/pin_persistence.rs"),
        read("crates/openless-core/src/remote_input_service.rs"),
        read("crates/openless-core/tests/remote_input_contract.rs"),
        read("src-tauri/src/core_adapters.rs"),
        read("src-tauri/src/coordinator.rs"),
        read("src-tauri/src/commands/remote_input.rs"),
        read("src-tauri/Cargo.toml"),
        read("../../.github/workflows/ci.yml"),
    ])

for (const token of ["O_NOFOLLOW", "O_NONBLOCK", "O_CLOEXEC"]) {
    assert.match(pinModule, new RegExp(`custom_flags\\([^)]*${token}`), `Unix open must use ${token}`)
}
assert.match(pinModule, /file\.metadata\(\)/, "validation must fstat the opened file")
assert.match(pinModule, /file\.set_permissions\(/, "permission repair must use the opened file")
assert.match(pinModule, /\.take\(MAX_PIN_FILE_BYTES \+ 1\)/, "reads must remain bounded after fstat")

for (const token of [
    "FILE_FLAG_OPEN_REPARSE_POINT",
    "GetFileInformationByHandle",
    "GetFileType",
    "FILE_ATTRIBUTE_REPARSE_POINT",
    "nNumberOfLinks",
    "ReplaceFileW",
    "MoveFileExW",
]) {
    assert.match(pinModule, new RegExp(token), `Windows implementation must use ${token}`)
}
assert.doesNotMatch(
    pinModule,
    /remove_file\(path\)/,
    "replacement must never delete the destination before installing the new PIN",
)
assert.match(pinModule, /backup_path/, "Windows replacement must retain a rollback path")

assert.match(appCargo, /"Win32_Storage_FileSystem"/, "Windows file APIs must be enabled")
assert.match(
    pinModule,
    /#\[cfg\(test\)\][\s\S]*mod tests[\s\S]*hard_link_pin_path_is_rejected/,
    "PIN persistence security tests must remain in the owning Tauri module",
)
assert.match(
    ci,
    /if: runner\.os != 'Windows'[\s\S]*cargo test --locked --manifest-path src-tauri\/Cargo\.toml --lib/,
    "non-Windows CI must execute the owning Tauri module tests",
)
assert.match(
    ci,
    /if: runner\.os == 'Windows'[\s\S]*cargo test --locked --manifest-path src-tauri\/Cargo\.toml --lib --no-run/,
    "Windows CI must at least compile the owning Tauri module tests when native runtime DLLs prevent execution",
)

const regenerate = coreRemote.match(
    /async fn regenerate_pairing_pin_inner[\s\S]*?async fn authenticate_inner/,
)?.[0]
assert.ok(regenerate, "Core pairing PIN transaction must be present")
assert.match(
    regenerate,
    /persist_pairing_pin\(pin\.clone\(\)\)[\s\S]*?\.await[\s\S]*?state\.pairing_pin = Some\(pin\);[\s\S]*?if restart[\s\S]*?stop_server_and_sessions\(\)\.await\?;[\s\S]*?start_server\(port\)\.await\?;/,
    "Core must persist the new PIN before committing memory and restarting the transport",
)
assert.match(
    coreContract,
    /failed_pin_persistence_keeps_the_committed_pin_and_server_state[\s\S]*?reject_persist\.store\(true[\s\S]*?regenerate_pairing_pin\(\)\.await\.unwrap_err\(\)[\s\S]*?old_pin[\s\S]*?status\(\)\.unwrap\(\)\.running[\s\S]*?start_count\.load\(Ordering::Acquire\), 1/,
    "Core contract tests must prove persistence failure preserves the committed PIN and running server",
)
assert.match(
    tauriAdapter,
    /impl openless_core::RemoteInputRuntimeAdapter for TauriRemoteInputRuntimeAdapter[\s\S]*?fn persist_pairing_pin[\s\S]*?crate::remote_server::save_pin/,
    "Tauri adapter must delegate pairing PIN persistence to the hardened atomic file implementation",
)
assert.doesNotMatch(
    coordinator,
    /regenerate_remote_pin|persist_and_commit_remote_pin|remote_server_handle|pairing_pin/,
    "Coordinator must not regain remote input PIN or transport ownership",
)
assert.match(
    command,
    /pub async fn regenerate_remote_pin[\s\S]*?-> Result<String, String>[\s\S]*?regenerate_pairing_pin\(\)[\s\S]*?\.await[\s\S]*?read_pairing_pin\(\)/,
    "Tauri command must surface Core reset failures before returning the committed PIN",
)

console.log("PIN persistence security contract passed")
