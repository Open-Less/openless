import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';
import yaml from 'js-yaml';

const ci = yaml.load(
  readFileSync(new URL('../../../.github/workflows/ci.yml', import.meta.url), 'utf8'),
);
const stable = ci.jobs['cross-platform'];
const step = (name) => {
  const value = stable.steps.find((candidate) => candidate.name === name);
  assert.ok(value, `retain ${name}`);
  return value;
};
for (const name of [
  'Run isolated Windows hang diagnostics tests (off/on)',
  'Run owner-thread target ownership tests',
  'Run real Windows runtime ownership lifecycle',
  'Run native recorder module tests (Windows)',
]) {
  assert.equal(
    step(name).env.CARGO_TARGET_DIR,
    '${{ github.workspace }}/openless-all/app/src-tauri/target',
  );
}
assert.match(
  step('Run Codex writable-roots sandbox contract').run,
  /--target-dir src-tauri\/target /,
);
assert.match(step('Run Windows audio COM lifecycle tests').run, /--target-dir src-tauri\/target /);
assert.match(
  step('Run Rust-only backend unit tests (Windows)').run,
  /--target-dir src-tauri\/target /,
);
assert.match(
  step('Compile opt-in Windows hang diagnostics host').run,
  /--profile test.*--lib --bins/,
);
assert.equal(step('Run real Windows runtime ownership lifecycle').if, "runner.os == 'Windows'");
assert.equal(step('Run native recorder module tests (Windows)').if, "runner.os == 'Windows'");
assert.match(step('Record runner resources and compiler limits').run, /os\.totalmem\(\)/);
const input = ci.on.workflow_dispatch.inputs.windows_build_jobs;
assert.equal(input.type, 'choice');
assert.equal(input.default, '2');
assert.deepEqual(input.options, ['2', '4']);
for (const key of ['CARGO_BUILD_JOBS', 'CMAKE_BUILD_PARALLEL_LEVEL']) {
  assert.equal(
    stable.env[key],
    "${{ matrix.label == 'Windows' && github.event_name == 'workflow_dispatch' && inputs.windows_build_jobs || '2' }}",
  );
  assert.equal(
    ci.jobs['windows-msrv'].env[key],
    "${{ github.event_name == 'workflow_dispatch' && inputs.windows_build_jobs || '2' }}",
  );
  assert.equal(ci.jobs['macos-msrv'].env[key], 2, 'do not raise the memory-sensitive Mac limit');
}
// Exercise the actual expressions: push/PR cannot accidentally opt into 4,
// and selecting 4 on a manual all-platform run must not increase Mac pressure.
for (const [event, label, selected, expected] of [
  ['push', 'Windows', '4', '2'],
  ['pull_request', 'Windows', '4', '2'],
  ['workflow_dispatch', 'Windows', '2', '2'],
  ['workflow_dispatch', 'Windows', '4', '4'],
  ['workflow_dispatch', 'macOS', '4', '2'],
  ['workflow_dispatch', 'Windows', '', '2'],
]) {
  const expression = stable.env.CARGO_BUILD_JOBS.slice(3, -2).trim();
  assert.equal(
    vm.runInNewContext(expression, {
      github: { event_name: event },
      matrix: { label },
      inputs: { windows_build_jobs: selected },
    }),
    expected,
    `${event}/${label}/${selected}`,
  );
}
assert.ok(
  ci.concurrency.group.includes("inputs.windows_build_jobs || '2'"),
  'A/B requests must not cancel each other',
);
const python = process.platform === 'win32' ? 'python' : 'python3';
const tests = spawnSync(
  python,
  [fileURLToPath(new URL('./test_ci_harness_cache.py', import.meta.url))],
  { encoding: 'utf8' },
);
assert.ifError(tests.error);
assert.equal(tests.status, 0, tests.stdout + tests.stderr);
console.log(
  'CI target reuse, preserved native harness modes and controlled parallelism contracts passed',
);
