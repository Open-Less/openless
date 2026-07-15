import assert from "node:assert/strict"
import { readFile } from "node:fs/promises"

const read = (path) => readFile(new URL(`../${path}`, import.meta.url), "utf8")

const [pinModule, coordinator, command, appCargo, backendCargo, backendTests, ci] =
    await Promise.all([
        read("src-tauri/src/remote_server/pin_persistence.rs"),
        read("src-tauri/src/coordinator.rs"),
        read("src-tauri/src/commands/remote_input.rs"),
        read("src-tauri/Cargo.toml"),
        read("src-tauri/backend-tests/Cargo.toml"),
        read("src-tauri/backend-tests/tests/backend_rust.rs"),
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

for (const cargo of [appCargo, backendCargo]) {
    assert.match(cargo, /"Win32_Storage_FileSystem"/, "Windows file APIs must be enabled")
}
assert.match(
    backendTests,
    /src\/remote_server\/pin_persistence\.rs/,
    "the Rust-only Windows harness must execute PIN persistence tests",
)
assert.match(
    ci,
    /if: runner\.os == 'Windows'[\s\S]*cargo test --manifest-path src-tauri\/backend-tests\/Cargo\.toml/,
    "Windows CI must execute the Rust-only PIN tests",
)

const assertCoordinatorContract = (source) => {
    const regenerate = source.match(
        /pub fn regenerate_remote_pin[\s\S]*?\r?\n    }\r?\n\r?\n    #\[cfg\(not\(mobile\)\)\]/,
    )?.[0]
    assert.ok(regenerate, "Coordinator regenerate implementation must be present")
    assert.match(regenerate, /-> Result<String, String>/, "reset must surface persistence errors")
    assert.match(regenerate, /persist_and_commit_remote_pin[\s\S]*save_pin[\s\S]*refresh_remote_server/, "reset must delegate persistence and state commit as one transaction")
    const transaction = source.match(
        /fn persist_and_commit_remote_pin[\s\S]*?\r?\n}\r?\n\r?\nimpl Coordinator/,
    )?.[0]
    assert.ok(transaction, "PIN reset transaction helper must be present")
    assert.match(transaction, /persist\(&pin\)\?;[\s\S]*\*slot\.lock\(\) = Some\(pin\.clone\(\)\);[\s\S]*refresh\(\)/, "persist must succeed before memory commit and server refresh")
}

assertCoordinatorContract(coordinator)
assertCoordinatorContract(coordinator.replace(/\r?\n/g, "\r\n"))
assert.match(command, /regenerate_remote_pin[\s\S]*-> Result<String, String>/, "Tauri command must reject on reset failure")

console.log("PIN persistence security contract passed")
