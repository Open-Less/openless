import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
if (process.platform !== 'darwin') {
  console.log('AppKit guide tests run on macOS');
} else {
  const app = resolve(dirname(fileURLToPath(import.meta.url)), '..');
  const output = join(mkdtempSync(join(tmpdir(), 'openless-guide-test-')), 'guide-test');
  execFileSync(
    'xcrun',
    [
      'clang',
      '-fobjc-arc',
      '-Werror',
      '-Wno-deprecated-declarations',
      '-framework',
      'AppKit',
      '-framework',
      'ApplicationServices',
      join(app, 'src-tauri/tests/accessibility-guide.m'),
      '-o',
      output,
    ],
    { stdio: 'inherit' },
  );
  execFileSync(output, [], { stdio: 'inherit' });
}
