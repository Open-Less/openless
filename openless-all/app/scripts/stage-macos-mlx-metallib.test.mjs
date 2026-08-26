import assert from "node:assert/strict"
import {
  existsSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs"
import { tmpdir } from "node:os"
import { join, resolve } from "node:path"
import { spawnSync } from "node:child_process"
import { fileURLToPath } from "node:url"

const appRoot = resolve(fileURLToPath(new URL("..", import.meta.url)))
const script = join(appRoot, "scripts", "stage-macos-mlx-metallib.mjs")

function candidate(root, suffix, content) {
  const path = join(root, `qwen3-asr-rs-${suffix}`, "out", "lib", "mlx.metallib")
  mkdirSync(resolve(path, ".."), { recursive: true })
  writeFileSync(path, content)
  return path
}

function run(buildRoot, output) {
  return spawnSync(process.execPath, [script, "--build-root", buildRoot, "--output", output], {
    cwd: appRoot,
    encoding: "utf8",
  })
}

const root = mkdtempSync(join(tmpdir(), "openless-mlx-stage-test-"))
try {
  const buildRoot = join(root, "build")
  const output = join(root, "staged", "mlx.metallib")
  mkdirSync(buildRoot, { recursive: true })
  mkdirSync(resolve(output, ".."), { recursive: true })

  writeFileSync(output, "stale", { flag: "w" })
  const missing = run(buildRoot, output)
  assert.notEqual(missing.status, 0)
  assert.match(missing.stderr, /未找到 MLX metallib/)
  assert.equal(existsSync(output), false)

  candidate(buildRoot, "one", "kernel-a")
  const single = run(buildRoot, output)
  assert.equal(single.status, 0, single.stderr)
  assert.equal(readFileSync(output, "utf8"), "kernel-a")

  candidate(buildRoot, "same", "kernel-a")
  const identical = run(buildRoot, output)
  assert.equal(identical.status, 0, identical.stderr)
  assert.equal(readFileSync(output, "utf8"), "kernel-a")

  candidate(buildRoot, "conflict", "kernel-b")
  const conflict = run(buildRoot, output)
  assert.notEqual(conflict.status, 0)
  assert.match(conflict.stderr, /内容冲突/)
  assert.match(conflict.stderr, /qwen3-asr-rs-one/)
  assert.match(conflict.stderr, /qwen3-asr-rs-conflict/)
} finally {
  rmSync(root, { recursive: true, force: true })
}

console.log("stage-macos-mlx-metallib tests passed")
