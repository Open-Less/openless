import { referenceBadgeView, updateReferenceBadge } from './referenceBadgeView';

const assert = {
  deepEqual(actual: unknown, expected: unknown) {
    if (JSON.stringify(actual) !== JSON.stringify(expected)) {
      throw new Error(`Expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
    }
  },
  equal(actual: unknown, expected: unknown) {
    if (actual !== expected) throw new Error(`Expected ${expected}, got ${actual}`);
  },
};

const session = { state: 'recording', sessionId: 'first' } as const;
assert.equal(referenceBadgeView(updateReferenceBadge(null, session, 0), 0), null);

const counted = updateReferenceBadge(null, { ...session, referenceCount: 2 }, 0);
assert.deepEqual(referenceBadgeView(counted, 0), { kind: 'count', count: 2 });
const processing = updateReferenceBadge(counted, { ...session, state: 'polishing', referenceCount: 2 }, 20);
assert.deepEqual(referenceBadgeView(processing, 20), { kind: 'count', count: 2 });

const tooLarge = { ...session, referenceCount: 2, referenceNotice: 'tooLarge' } as const;
const warning = updateReferenceBadge(counted, tooLarge, 100);
assert.deepEqual(referenceBadgeView(warning, 4_099), { kind: 'notice', notice: 'tooLarge' });
const repeated = updateReferenceBadge(warning, tooLarge, 3_000);
assert.deepEqual(referenceBadgeView(repeated, 4_100), { kind: 'count', count: 2 });
const persistent = updateReferenceBadge(repeated, tooLarge, 4_200);
assert.deepEqual(referenceBadgeView(persistent, 4_200), { kind: 'count', count: 2 });

const another = updateReferenceBadge(warning, { ...session, referenceNotice: 'captureFailed' }, 3_000);
assert.deepEqual(referenceBadgeView(another, 6_999), { kind: 'notice', notice: 'captureFailed' });
assert.equal(referenceBadgeView(another, 7_000), null);

const cleared = updateReferenceBadge(warning, { ...session, referenceCount: 3 }, 3_100);
assert.deepEqual(referenceBadgeView(cleared, 3_100), { kind: 'notice', notice: 'tooLarge' });
const sameAfterClear = updateReferenceBadge(cleared, tooLarge, 3_200);
assert.deepEqual(referenceBadgeView(sameAfterClear, 4_100), { kind: 'count', count: 2 });
assert.equal(updateReferenceBadge(warning, { ...session, sessionId: 'next' }, 200), null);
assert.equal(updateReferenceBadge(warning, { ...tooLarge, state: 'idle' }, 200), null);
