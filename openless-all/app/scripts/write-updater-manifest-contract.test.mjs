import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const script = fileURLToPath(new URL('./write-updater-manifest.mjs', import.meta.url));
const packageJson = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8'));
const releaseTag = 'v2.0.2-Beta.3-tauri';
const tempDir = mkdtempSync(join(tmpdir(), 'openless-updater-'));
const assetName = `OpenLess_${packageJson.version}_arm64-v8a.apk`;

try {
  writeFileSync(join(tempDir, assetName), 'fixture');
  writeFileSync(join(tempDir, `${assetName}.sig`), 'signature');
  execFileSync(process.execPath, [script], {
    env: {
      ...process.env,
      OPENLESS_UPDATE_APK_DIR: tempDir,
      OPENLESS_UPDATE_ARCH: 'aarch64',
      OPENLESS_UPDATE_CDN_BASE_URL: 'https://cdn.example/',
      OPENLESS_UPDATE_TARGET: 'android',
      OPENLESS_RELEASE_CHANNEL: 'beta',
      OPENLESS_RELEASE_TAG: releaseTag,
    },
    stdio: 'pipe',
  });

  const expected =
    `https://cdn.example/releases/${encodeURIComponent(releaseTag)}/` +
    encodeURIComponent(assetName);
  for (const suffix of ['', '-mirror']) {
    const manifest = JSON.parse(
      readFileSync(join(tempDir, `latest-android-aarch64-beta${suffix}.json`), 'utf8'),
    );
    assert.equal(manifest.url, expected);
  }

  const stableTag = 'v2.0.1-tauri';
  execFileSync(process.execPath, [script], {
    env: {
      ...process.env,
      OPENLESS_UPDATE_APK_DIR: tempDir,
      OPENLESS_UPDATE_ARCH: 'aarch64',
      OPENLESS_UPDATE_CDN_BASE_URL: 'https://cdn.example/',
      OPENLESS_UPDATE_TARGET: 'android',
      OPENLESS_RELEASE_CHANNEL: 'stable',
      OPENLESS_RELEASE_TAG: stableTag,
    },
    stdio: 'pipe',
  });
  const stableExpected =
    `https://cdn.example/releases/${encodeURIComponent(stableTag)}/` +
    encodeURIComponent(assetName);
  for (const suffix of ['', '-mirror']) {
    const manifest = JSON.parse(
      readFileSync(join(tempDir, `latest-android-aarch64${suffix}.json`), 'utf8'),
    );
    assert.equal(manifest.url, stableExpected);
  }
} finally {
  rmSync(tempDir, { recursive: true, force: true });
}

console.log('write-updater-manifest-contract.test.mjs passed');
