import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import yaml from 'js-yaml';

const workflow = yaml.load(
  await readFile(new URL('../../../.github/workflows/ci.yml', import.meta.url), 'utf8'),
);

const runnerInput = workflow.on.workflow_dispatch.inputs.macos_runner;
assert.equal(runnerInput.type, 'choice');
assert.equal(runnerInput.default, 'macos-latest');
assert.deepEqual(runnerInput.options, ['macos-latest', 'macos-15-intel']);

const crossPlatform = workflow.jobs['cross-platform'];
assert.equal(
  crossPlatform['runs-on'],
  "${{ matrix.label == 'macOS' && github.event_name == 'workflow_dispatch' && inputs.macos_runner || matrix.os }}",
  'Only manual macOS checks may override the default matrix runner',
);
assert.equal(
  workflow.jobs['macos-msrv']['runs-on'],
  "${{ github.event_name == 'workflow_dispatch' && inputs.macos_runner || 'macos-latest' }}",
  'MSRV must share the manual fallback and keep its default for push/PR events',
);
assert.ok(crossPlatform.strategy.matrix.include.includes('"os":"windows-latest"'));
assert.equal(crossPlatform.strategy['fail-fast'], false);
for (const gate of [
  'Build frontend and run frontend/contract tests',
  'Run Codex writable-roots sandbox contract',
  'Run Rust backend unit tests',
  'Run native recorder module tests (Windows)',
]) {
  assert.ok(
    crossPlatform.steps.some((step) => step.name === gate),
    `${gate} must remain enabled`,
  );
}

console.log('ci-macos-runner-contract.test.mjs passed');
