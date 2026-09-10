import {
  isStylePackZipDialogCancellation,
  pickStylePackZipTargetPath,
  stylePackZipFileName,
} from './stylePackZip';

function assertEqual<T>(actual: T, expected: T, message: string): void {
  if (actual !== expected) {
    throw new Error(`${message}: expected ${String(expected)}, received ${String(actual)}`);
  }
}

assertEqual(
  stylePackZipFileName('会议记录', '1.2.0'),
  '会议记录-v1.2.0.zip',
  'marketplace downloads should include a readable versioned file name',
);

assertEqual(
  stylePackZipFileName('Quarterly: Review / Final', '1/2'),
  'quarterly-review-final-v1-2.zip',
  'file names should remove path separators and collapse punctuation gaps',
);

assertEqual(
  stylePackZipFileName('My Pack'),
  'my-pack.zip',
  'local style exports should preserve their existing unversioned naming',
);

assertEqual(
  stylePackZipFileName('  ', '  '),
  'style-pack.zip',
  'blank metadata should still produce a usable ZIP file name',
);

assertEqual(
  isStylePackZipDialogCancellation(new Error('File picker cancelled')),
  true,
  'Android dialog cancellation should be treated as a silent user action',
);

assertEqual(
  isStylePackZipDialogCancellation('Failed to save file'),
  false,
  'real save failures should still be surfaced',
);

assertEqual(
  await pickStylePackZipTargetPath('demo.zip', false),
  '~/Downloads/demo.zip',
  'browser development mode should keep the existing mock destination',
);

console.log('stylePackZip tests passed');
