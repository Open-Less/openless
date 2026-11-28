// Exercise the workflow's real trial commands without compiling or installing.
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { runInNewContext } from 'node:vm';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const workflow = readFileSync(join(root, '.github/workflows/release-tauri.yml'), 'utf8');
const job = workflow.split('\n  dev-macos:\n')[1].split('\n  prepare-release:\n')[0];
const condition = job.match(/^    if: (.+)$/m)[1];
for (const [event, dev, expected] of [
  ['schedule', false, true],
  ['workflow_dispatch', true, true],
  ['workflow_dispatch', false, false],
  ['push', true, false],
  ['pull_request', true, false],
]) {
  assert.equal(
    runInNewContext(condition, { github: { event_name: event }, inputs: { dev_build: dev } }),
    expected,
    `${event} dev=${dev}: only scheduled warming and explicit Dev builds may use this cache`,
  );
}

const configure = job.indexOf('- name: Configure Dev build environment');
const cache = job.indexOf('- name: Cache Dev Rust dependencies');
const compile = job.indexOf('- name: Compile and package Dev');
assert.ok(configure >= 0 && configure < cache && cache < compile,
  'the cache must fingerprint the same profile and compiler wrapper used by the build');
assert.match(job, /shared-key: tauri-macos-dev/);
assert.doesNotMatch(job, /^\s+(?:cache-all-crates|cache-workspace-crates): true/m,
  'source changes must rebuild workspace crates, without caching trial binaries');
const build = job.slice(compile).match(/        run: \|\n([\s\S]*?)(?=      - name:)/)[1]
  .split('\n').map((line) => line.slice(10)).join('\n');
const envBlock = job.split('    env:\n')[1].split('    steps:\n')[0];
const buildEnv = Object.fromEntries([...envBlock.matchAll(/^      ([A-Z_]+): '([^']*)'$/gm)]
  .map((match) => [match[1], match[2]]));
assert.equal(buildEnv.CARGO_BUILD_JOBS, '2');
assert.equal(buildEnv.CMAKE_BUILD_PARALLEL_LEVEL, '2');
assert.equal(buildEnv.CARGO_PROFILE_RELEASE_LTO, 'false');
assert.doesNotMatch(workflow.split('\n  prepare-release:\n')[1], /CARGO_PROFILE_RELEASE_LTO/,
  'normal publishing must retain its existing optimization profile');

const sandbox = mkdtempSync(join(process.env.TMPDIR || tmpdir(), 'openless-dev-cache-'));
try {
  const bin = join(sandbox, 'bin');
  mkdirSync(bin);
  writeFileSync(join(bin, 'npm'), '#!/bin/bash\nprintf "%s\\n" "$*" "$CARGO_BUILD_JOBS:$CARGO_PROFILE_RELEASE_LTO:$CARGO_PROFILE_RELEASE_CODEGEN_UNITS:$CARGO_PROFILE_RELEASE_STRIP" "$RUSTC_WRAPPER" >> "$CAPTURE"\n', { mode: 0o755 });
  const githubEnv = join(sandbox, 'github-env');
  const configured = spawnSync('bash', ['../macos/scripts/macos-build-env.sh'], {
    cwd: join(root, 'openless-all/app'),
    env: { ...process.env, ...buildEnv, GITHUB_ENV: githubEnv },
    encoding: 'utf8',
  });
  assert.equal(configured.status, 0, configured.stderr);
  const persisted = Object.fromEntries(readFileSync(githubEnv, 'utf8').trim().split('\n')
    .map((line) => { const i = line.indexOf('='); return [line.slice(0, i), line.slice(i + 1)]; }));
  for (const official of ['true', 'false']) {
    const capture = join(sandbox, `npm-${official}`);
    const result = spawnSync('bash', ['-e', '-o', 'pipefail', '-c', build], {
      cwd: join(root, 'openless-all/app'),
      env: { ...process.env, ...buildEnv, ...persisted, PATH: `${bin}:${process.env.PATH}`,
        OFFICIAL_IDENTITY: official, CAPTURE: capture },
      encoding: 'utf8',
    });
    assert.equal(result.status, 0, result.stderr);
    const calls = readFileSync(capture, 'utf8');
    assert.match(calls, /--locked --timings/);
    assert.match(calls, /2:false:16:debuginfo/);
    assert.ok(calls.includes(persisted.RUSTC_WRAPPER));
    if (official === 'true') {
      assert.match(calls, /tauri\.trial\.conf\.json/);
      assert.doesNotMatch(calls, /--features dev-build/);
    } else {
      assert.match(calls, /--features dev-build/);
      assert.match(calls, /tauri\.dev\.conf\.json/);
    }
  }
} finally {
  rmSync(sandbox, { recursive: true, force: true });
}
console.log('Dev cache workflow, profile propagation and both trial identities passed');
