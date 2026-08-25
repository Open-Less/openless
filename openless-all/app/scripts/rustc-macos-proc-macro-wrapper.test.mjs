import assert from "node:assert/strict"
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { join, resolve } from "node:path"
import { spawnSync } from "node:child_process"
import { fileURLToPath } from "node:url"

if (process.platform === "win32") {
  console.log("rustc macOS wrapper test skipped on Windows")
  process.exit(0)
}

const appRoot = resolve(fileURLToPath(new URL("..", import.meta.url)))
const wrapper = join(appRoot, "scripts", "rustc-macos-proc-macro-wrapper.sh")
const root = mkdtempSync(join(tmpdir(), "openless-rustc-wrapper-test-"))
const fakeRustc = join(root, "fake-rustc.sh")

writeFileSync(
  fakeRustc,
  `#!/usr/bin/env bash
printf 'deployment=%s\\n' "\${MACOSX_DEPLOYMENT_TARGET-unset}"
printf 'arg=<%s>\\n' "$@"
`,
)
chmodSync(fakeRustc, 0o755)

function run(args) {
  return spawnSync("bash", [wrapper, fakeRustc, ...args], {
    cwd: appRoot,
    encoding: "utf8",
    env: { ...process.env, MACOSX_DEPLOYMENT_TARGET: "14.0" },
  })
}

try {
  for (const args of [
    ["--crate-type=proc-macro", "--crate-name", "inline"],
    ["--crate-type", "proc-macro", "--crate-name", "split"],
  ]) {
    const result = run(args)
    assert.equal(result.status, 0, result.stderr)
    assert.match(result.stdout, /^deployment=unset/m)
    assert.deepEqual(
      result.stdout.match(/^arg=<.*>$/gm),
      args.map(arg => `arg=<${arg}>`),
    )
  }

  const regularArgs = ["--crate-type=lib", "--crate-name", "regular"]
  const regular = run(regularArgs)
  assert.equal(regular.status, 0, regular.stderr)
  assert.match(regular.stdout, /^deployment=14\.0/m)
  assert.deepEqual(
    regular.stdout.match(/^arg=<.*>$/gm),
    regularArgs.map(arg => `arg=<${arg}>`),
  )
} finally {
  rmSync(root, { recursive: true, force: true })
}

console.log("rustc macOS proc-macro wrapper tests passed")
