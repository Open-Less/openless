import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, copyFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
const root = mkdtempSync(join(tmpdir(), 'openless-overlay-manifest-'));
try {
  mkdirSync(join(root, 'scripts'));
  const script = join(root, 'scripts/merge-android-overlay-manifest.mjs');
  copyFileSync(new URL('./merge-android-overlay-manifest.mjs', import.meta.url), script);
  const directory = join(root, 'src-tauri/gen/android/app/src/main');
  mkdirSync(directory, { recursive: true });
  const manifest = join(directory, 'AndroidManifest.xml');
  const filter =
    '<intent-filter><action android:name="android.intent.action.MAIN"/><category android:name="android.intent.category.LAUNCHER"/></intent-filter>';
  const other = '<intent-filter><action android:name="example.OTHER"/></intent-filter>';
  for (const source of ['.MainActivity', '.MicrophonePermissionActivity']) {
    writeFileSync(
      manifest,
      `<manifest xmlns:android="http://schemas.android.com/apk/res/android"><application><activity android:name=".MainActivity" android:exported="true">${source === '.MainActivity' ? filter : ''}${other}</activity>${source === '.MicrophonePermissionActivity' ? `<activity android:name="${source}" android:exported="true">${filter}</activity>` : ''}</application></manifest>`,
    );
    const run = () => {
      const result = spawnSync(process.execPath, [script], { encoding: 'utf8' });
      assert.equal(result.status, 0, result.stderr);
      return readFileSync(manifest, 'utf8');
    };
    const first = run();
    assert.equal(run(), first, 'repeated merge must preserve the complete document');
    assert.equal(run(), first);
    assert.ok(first.includes(other), 'unrelated filter must survive');
    const parsed = spawnSync(
      process.platform === 'win32' ? 'python' : 'python3',
      [
        '-c',
        `import sys,json,xml.etree.ElementTree as E
ns='{http://schemas.android.com/apk/res/android}'
root=E.fromstring(sys.stdin.read())
print(json.dumps([a.get(ns+'name') for a in root.findall('./application/activity') if any(c.get(ns+'name')=='android.intent.category.LAUNCHER' for c in a.findall('./intent-filter/category'))]))`,
      ],
      { input: first, encoding: 'utf8' },
    );
    assert.equal(parsed.status, 0, parsed.stderr);
    assert.deepEqual(JSON.parse(parsed.stdout), ['.OpenLessBackendWarmupActivity']);
  }
} finally {
  rmSync(root, { recursive: true, force: true });
}
console.log('overlay manifest merge tests passed');
