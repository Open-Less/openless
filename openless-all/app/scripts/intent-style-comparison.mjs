// Offline report rendering. Network requests live only in the explicitly ignored Rust test.
import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';

const fixtureUrl = new URL(
  '../crates/openless-core/tests/fixtures/intent-reconstruction-evaluation.json',
  import.meta.url,
);

export function comparisonTemplate() {
  const evaluation = JSON.parse(readFileSync(fixtureUrl, 'utf8'));
  return {
    schemaVersion: evaluation.schemaVersion,
    provenance: evaluation.provenance,
    baselineVersion: '1.3.0',
    candidateVersion: '1.5.0',
    semanticReview: 'pending human review; no real model requests have been made',
    cases: evaluation.cases.map((entry) => ({ ...entry, old: null, new: null })),
  };
}

const cell = (value) =>
  String(value ?? '')
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('|', '&#124;')
    .replaceAll('\r\n', '\n')
    .replaceAll('\n', '<br>');

function resultCell(result) {
  if (!result) return 'Pending real model run';
  if (result.errorCode) return `Request failed: ${cell(result.errorCode)}`;
  const source = result.sourceText ? `Source: ${cell(result.sourceText)}<br><br>` : '';
  return `${source}${cell(result.text)}${result.elapsedMs == null ? '' : `<br><br>${cell(result.elapsedMs)} ms`}`;
}

export function renderComparison(report) {
  if (report.schemaVersion !== 1 || !Array.isArray(report.cases)) {
    throw new Error('Expected comparison schemaVersion 1 and cases');
  }
  const lines = [
    `# Intent style comparison: ${cell(report.baselineVersion)} → ${cell(report.candidateVersion)}`,
    '',
    String(report.provenance ?? ''),
    '',
    `Review: ${String(report.semanticReview ?? 'pending human review')}`,
    '',
    '| Case / input | Must preserve / remove / structure | Old output | New output |',
    '| --- | --- | --- | --- |',
  ];
  for (const entry of report.cases) {
    for (const field of ['mustPreserve', 'mustRemove', 'structure']) {
      if (!Array.isArray(entry[field]) || !entry[field].length) {
        throw new Error(`${entry.id}: missing ${field}`);
      }
    }
    const expected = [
      `Preserve: ${entry.mustPreserve.join('; ')}`,
      `Remove: ${entry.mustRemove.join('; ')}`,
      `Structure: ${entry.structure.join('; ')}`,
    ]
      .map(cell)
      .join('<br>');
    lines.push(
      `| ${cell(entry.id)}<br>${cell(entry.operation)}<br>${cell(entry.input)} | ${expected} | ${resultCell(entry.old)} | ${resultCell(entry.new)} |`,
    );
  }
  return `${lines.join('\n')}\n`;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  if (args.length === 3 && args[0] === '--template') {
    const report = comparisonTemplate();
    writeFileSync(args[1], `${JSON.stringify(report, null, 2)}\n`, { flag: 'wx' });
    writeFileSync(args[2], renderComparison(report), { flag: 'wx' });
  } else if (args.length === 2) {
    writeFileSync(args[1], renderComparison(JSON.parse(readFileSync(args[0], 'utf8'))), {
      flag: 'wx',
    });
  } else {
    throw new Error(
      'Usage: node scripts/intent-style-comparison.mjs [--template] comparison.json comparison.md',
    );
  }
}
