import { readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

// These platform gates distinguish a full source run from a macOS-only dispatch.
// Every other job defined by that immutable source must also have succeeded.
export const REQUIRED_RELEASE_GATES = ['macOS checks', 'Windows checks', 'Android cargo check'];

export function verifyReleaseCi(report) {
  if (!Array.isArray(report.jobs) || report.total_count !== report.jobs.length)
    throw new Error('CI job report is incomplete');
  for (const name of REQUIRED_RELEASE_GATES) {
    if (
      !report.jobs.some(
        (job) => job.name === name && job.status === 'completed' && job.conclusion === 'success',
      )
    )
      throw new Error(`Required release gate did not pass: ${name}`);
  }
  for (const job of report.jobs) {
    if (job.status === 'completed' && job.conclusion === 'success') continue;
    if (
      job.status === 'completed' &&
      job.conclusion === 'skipped' &&
      job.name.endsWith(' / publish-linux-egui')
    )
      continue;
    throw new Error(`CI job did not pass: ${job.name}`);
  }
  return true;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  verifyReleaseCi(JSON.parse(readFileSync(process.argv[2], 'utf8')));
  console.log('Exact-source release CI gates passed');
}
