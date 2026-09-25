const assert = {
  equal(actual: unknown, expected: unknown) {
    if (actual !== expected) throw new Error(`Expected ${String(expected)}, got ${String(actual)}`);
  },
};
import { capsuleTranscriptFontSize, visibleCapsuleTranscript } from './capsuleTranscript';
assert.equal(capsuleTranscriptFontSize(undefined), 14);
assert.equal(capsuleTranscriptFontSize(NaN), 14);
assert.equal(capsuleTranscriptFontSize(50), 20);
assert.equal(capsuleTranscriptFontSize(0), 12);
assert.equal(visibleCapsuleTranscript('原文', false, 'recording', false), '');
assert.equal(visibleCapsuleTranscript('原文', true, 'recording', false), '原文');
assert.equal(visibleCapsuleTranscript('原文', true, 'polishing', false), '原文');
assert.equal(visibleCapsuleTranscript('原文', true, 'done', false), '');
assert.equal(visibleCapsuleTranscript('原文', true, 'recording', true), '');
console.log('capsuleTranscript tests passed');
