import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import { resolve } from "node:path"
import { fileURLToPath } from "node:url"

const appRoot = resolve(fileURLToPath(new URL("..", import.meta.url)))
const overlay = JSON.parse(
  readFileSync(resolve(appRoot, "src-tauri/tauri.macos-mlx.conf.json"), "utf8"),
)
const buildScript = readFileSync(resolve(appRoot, "scripts/build-mac.sh"), "utf8")

assert.equal(
  overlay.build.beforeBundleCommand,
  "node scripts/stage-macos-mlx-metallib.mjs",
)
assert.equal(
  overlay.bundle.macOS.files["MacOS/mlx.metallib"],
  "target/release/openless-mlx/mlx.metallib",
)
assert.match(buildScript, /arm64\)[\s\S]*tauri\.macos-mlx\.conf\.json/)
assert.doesNotMatch(buildScript, /--bundles app/)
assert.doesNotMatch(buildScript, /codesign --force/)
assert.doesNotMatch(buildScript, /hdiutil create/)
assert.match(buildScript, /OpenLess\.app\.tar\.gz/)
assert.match(buildScript, /stapler validate/)

console.log("macOS MLX bundle contract tests passed")
