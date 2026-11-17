import assert from 'node:assert/strict';
import { REQUIRED_RELEASE_GATES, verifyReleaseCi } from './verify-release-ci.mjs';
const jobs = REQUIRED_RELEASE_GATES.map((name) => ({
  name,
  status: 'completed',
  conclusion: 'success',
}));
const report = (jobs) => ({ total_count: jobs.length, jobs });
assert.equal(verifyReleaseCi(report(jobs)), true);
assert.equal(
  verifyReleaseCi(
    report([
      ...jobs,
      {
        name: 'Linux Core, egui and deb/rpm checks / publish-linux-egui',
        status: 'completed',
        conclusion: 'skipped',
      },
    ]),
  ),
  true,
);
assert.throws(() => verifyReleaseCi(report(jobs.slice(1))), /Required release gate/);
assert.throws(() => verifyReleaseCi({ ...report(jobs), total_count: 1000 }), /incomplete/);
assert.throws(
  () =>
    verifyReleaseCi(report(jobs.map((job, i) => (i ? job : { ...job, conclusion: 'skipped' })))),
  /Required release gate/,
);
assert.throws(
  () =>
    verifyReleaseCi(
      report([
        ...jobs,
        { name: 'Future required test', status: 'completed', conclusion: 'failure' },
      ]),
    ),
  /did not pass/,
);
console.log('release CI completeness and required gate checks passed');
