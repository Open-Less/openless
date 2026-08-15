import { nextFallbackCardHeightReport } from './insertFallbackLayout';

function assert(condition: boolean, message: string) {
  if (!condition) throw new Error(message);
}

function assertReport(
  actual: ReturnType<typeof nextFallbackCardHeightReport>,
  presentationId: number,
  height: number,
) {
  assert(actual?.presentationId === presentationId, 'presentation id should match');
  assert(actual?.height === height, 'height should be rounded up');
}

assertReport(nextFallbackCardHeightReport(null, 7, 181.2), 7, 182);
assert(
  nextFallbackCardHeightReport({ presentationId: 7, height: 182 }, 7, 181.2) === null,
  'same presentation and height should be deduplicated',
);
assertReport(
  nextFallbackCardHeightReport({ presentationId: 7, height: 182 }, 8, 181.2),
  8,
  182,
);
assert(
  nextFallbackCardHeightReport(null, 7, Number.NaN) === null,
  'non-finite height should be ignored',
);
assert(nextFallbackCardHeightReport(null, 7, 0) === null, 'non-positive height should be ignored');

console.log('insert fallback layout tests passed');
