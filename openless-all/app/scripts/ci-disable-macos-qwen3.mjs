import { spawnSync } from "node:child_process"
import { readFileSync, writeFileSync } from "node:fs"
import { dirname, resolve } from "node:path"
import { fileURLToPath } from "node:url"

const appRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..")
const cargoPath = resolve(appRoot, "src-tauri/Cargo.toml")
const lockPath = resolve(appRoot, "src-tauri/Cargo.lock")
const cargo = readFileSync(cargoPath, "utf8")
const dependency = /^qwen3-asr-rs\s*=\s*\{[^\n]+\}\r?\n/m

if (!dependency.test(cargo)) {
    throw new Error(`未找到 macOS-only qwen3-asr-rs 依赖：${cargoPath}`)
}

writeFileSync(cargoPath, cargo.replace(dependency, ""))
const lock = readFileSync(lockPath, "utf8")
if (!lock.includes('name = "qwen3-asr-rs"')) {
    throw new Error(`openless Cargo.lock package 未包含 qwen3-asr-rs：${lockPath}`)
}

const cargoResult = spawnSync("cargo", ["generate-lockfile", "--manifest-path", cargoPath], {
    cwd: appRoot,
    stdio: "inherit",
})
if (cargoResult.error) {
    throw cargoResult.error
}
if (cargoResult.status !== 0) {
    throw new Error(`cargo generate-lockfile 失败，退出码：${cargoResult.status}`)
}

const regeneratedLock = readFileSync(lockPath, "utf8")
if (regeneratedLock.includes('name = "qwen3-asr-rs"')) {
    throw new Error(`cargo generate-lockfile 后仍包含 qwen3-asr-rs：${lockPath}`)
}
console.log("[ci] disabled macOS-only qwen3-asr-rs dependency for this target")
