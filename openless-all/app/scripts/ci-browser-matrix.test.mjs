import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import yaml from 'js-yaml';
const workflow = yaml.load(
  readFileSync(new URL('../../../.github/workflows/ci.yml', import.meta.url), 'utf8'),
);
const browser = workflow.jobs['ui-motion'];
assert.deepEqual(browser.strategy.matrix.browser, ['chromium', 'webkit']);
assert.equal(browser.strategy['fail-fast'], false);
assert.ok(browser.steps.some((s) => s.run?.includes('--project=${{ matrix.browser }}')));
assert.ok(browser.steps.some((s) => s.with?.name === 'ui-motion-failure-${{ matrix.browser }}'));
const gate = workflow.jobs['ui-motion-result'];
assert.deepEqual(gate.needs, ['changes', 'ui-motion']);
assert.equal(gate.name, 'UI motion (Chromium + WebKit)');
for (const [result, required, expected] of [
  ['success', 'true', 0],
  ['skipped', 'false', 0],
  ['failure', 'true', 1],
  ['cancelled', 'true', 1],
  ['skipped', 'true', 1],
  ['', '', 1],
]) {
  const out = spawnSync('sh', ['-c', gate.steps[0].run], {
    env: { ...process.env, BROWSER_RESULT: result, FRONTEND_REQUIRED: required },
  });
  assert.equal(
    out.status,
    expected,
    `${result}/${required} must not incorrectly report a green browser gate`,
  );
}
console.log('parallel browser coverage and aggregate failure propagation passed');
