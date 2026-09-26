import assert from 'node:assert/strict';
import {
  chmodSync,
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';

const root = mkdtempSync(join(tmpdir(), 'openless-signing-'));
try {
  mkdirSync(join(root, 'scripts'));
  mkdirSync(join(root, 'src-tauri/gen/android/app'), { recursive: true });
  const script = join(root, 'scripts/configure-android-release-signing.mjs');
  copyFileSync(new URL('./configure-android-release-signing.mjs', import.meta.url), script);
  const gradle = join(root, 'src-tauri/gen/android/app/build.gradle.kts');
  const env = {
    ...process.env,
    ANDROID_KEYSTORE_BASE64: Buffer.from('fixture-keystore').toString('base64'),
    ANDROID_KEYSTORE_PASSWORD: 'fixture-store-$"}\\secret',
    ANDROID_KEY_ALIAS: 'fixture-alias',
    ANDROID_KEY_PASSWORD: 'fixture-key-secret',
  };
  for (const template of [
    'android {\n buildTypes { getByName("release") { isMinifyEnabled = false } }\n}',
    'android {\n namespace = "test"\n}',
    'android {\n signingConfigs { create("debug") {} }\n buildTypes {}\n}',
    'android {\n signingConfigs { create("openlessRelease") { storePassword = "old-}secret" } }\n buildTypes { getByName("release") {} }\n}',
  ]) {
    writeFileSync(gradle, template);
    const keystore = join(root, 'src-tauri/gen/android/openless-release.keystore');
    writeFileSync(keystore, 'old-keystore');
    chmodSync(keystore, 0o644);
    const run = () => spawnSync(process.execPath, [script], { env, encoding: 'utf8' });
    const first = run();
    assert.equal(first.status, 0, first.stderr);
    const content = readFileSync(gradle, 'utf8');
    for (const name of ['ANDROID_KEYSTORE_PASSWORD', 'ANDROID_KEY_ALIAS', 'ANDROID_KEY_PASSWORD']) {
      assert.ok(content.includes(`System.getenv("${name}")`));
      assert.ok(!content.includes(env[name]), `${name} must not be embedded in cacheable source`);
      assert.ok(!first.stdout.includes(env[name]));
    }
    assert.ok(!content.includes('old-}secret'));
    assert.ok(
      content.indexOf('create("openlessRelease")') <
        content.indexOf('signingConfigs.getByName("openlessRelease")'),
    );
    if (process.platform !== 'win32') assert.equal(statSync(keystore).mode & 0o777, 0o600);
    assert.equal(content.match(/create\("openlessRelease"\)/g)?.length, 1);
    assert.equal(
      content.match(/signingConfig = signingConfigs.getByName\("openlessRelease"\)/g)?.length,
      1,
    );
    const second = run();
    assert.equal(second.status, 0, second.stderr);
    assert.equal(readFileSync(gradle, 'utf8'), content);
    assert.equal(
      readFileSync(join(root, 'src-tauri/gen/android/openless-release.keystore'), 'utf8'),
      'fixture-keystore',
    );
  }
} finally {
  rmSync(root, { recursive: true, force: true });
}
console.log('Android release signing keeps credentials out of Gradle source');
