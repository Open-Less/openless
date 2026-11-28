import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { comparisonTemplate, renderComparison } from './intent-style-comparison.mjs';

const report = comparisonTemplate();
assert.equal(report.cases.length, 16);
assert.equal(report.baselineVersion, '1.3.0');
assert.equal(report.candidateVersion, '1.5.0');
const correction = report.cases.find((entry) => entry.id === 'partial-correction');
assert.ok(correction.mustPreserve.includes('启用自动保存的最终肯定要求'));
assert.ok(correction.mustRemove.includes('不要启用自动保存的已撤回禁止'));
const constraints = report.cases.find((entry) => entry.id === 'local-constraint');
assert.ok(constraints.mustPreserve.includes('访客禁止删除'));
assert.ok(constraints.mustPreserve.includes('管理员仅在备份成功后允许删除'));
assert.ok(report.cases.every((entry) => entry.old === null && entry.new === null));
const pending = renderComparison(report);
assert.equal(pending.match(/Pending real model run/g).length, 32);
assert.ok(pending.includes('pending human review'));
report.cases[0].old = { text: '<old> | before\nsecond', elapsedMs: 12 };
report.cases[0].new = { text: 'after', sourceText: '源文', elapsedMs: 15 };
report.credentials = { 'ark.api_key': 'must-never-render' };
const rendered = renderComparison(report);
assert.ok(rendered.includes('&lt;old&gt; &#124; before<br>second'));
assert.ok(rendered.includes('Source: 源文<br><br>after'));
assert.ok(rendered.includes('12 ms'));
assert.ok(!rendered.includes(report.credentials['ark.api_key']));
assert.ok(!rendered.includes('credentials'));
assert.ok(!rendered.includes('ark.api_key'));
report.cases[0].old = { errorCode: 'Provider' };
assert.ok(renderComparison(report).includes('Request failed: Provider'));
assert.throws(() => renderComparison({ schemaVersion: 2, cases: [] }));
const root = new URL('../crates/openless-core/src/builtin_styles/', import.meta.url);
const pack = JSON.parse(readFileSync(new URL('intent-reconstruction-v1.json', root), 'utf8'));
const shared = readFileSync(new URL('intent-reconstruction-shared-v1.md', root), 'utf8').trimEnd();
assert.equal(pack.version, report.candidateVersion);
assert.equal(pack.prompt, shared);
assert.equal(pack.selectionPrompt, shared);
const source = readFileSync(
  new URL('../crates/openless-core/src/intent_style_tests.rs', import.meta.url),
  'utf8',
);
assert.match(source, /#\[ignore = "real configured-channel requests/);
assert.match(source, /OPENLESS_INTENT_COMPARISON_APPROVED/);
assert.match(source, /create_new\(true\)/, 'preserve earlier real model evidence');
console.log('intent-style-comparison.test.mjs passed');
