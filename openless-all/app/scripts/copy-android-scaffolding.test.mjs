#!/usr/bin/env node
import assert from 'node:assert/strict';
import {
  copyFileSync,
  cpSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import {
  formatStringResource,
  hasStringResource,
  mergeMissingStringResources,
  mergeKeyboardSettingsDebugActivity,
  SHIZUKU_STRINGS_BY_LOCALE,
} from './copy-android-scaffolding.mjs';

const baseXml = `<?xml version="1.0" encoding="utf-8"?>
<resources>
    <string name="app_name">OpenLess</string>
</resources>`;

const partialXml = `<?xml version="1.0" encoding="utf-8"?>
<resources>
    <string name="openless_shizuku_permission_rationale">exists</string>
</resources>`;

const mergedAll = mergeMissingStringResources(baseXml, SHIZUKU_STRINGS_BY_LOCALE.values);
assert.equal(mergedAll.changed, true);
assert.ok(hasStringResource(mergedAll.content, 'openless_shizuku_permission_rationale'));
assert.ok(hasStringResource(mergedAll.content, 'openless_shizuku_permission_blocked'));
assert.ok(hasStringResource(mergedAll.content, 'openless_shizuku_open_manager'));

const mergedPartial = mergeMissingStringResources(partialXml, SHIZUKU_STRINGS_BY_LOCALE.values);
assert.equal(mergedPartial.changed, true);
assert.ok(hasStringResource(mergedPartial.content, 'openless_shizuku_open_manager'));
assert.ok(
  !mergedPartial.content.includes(
    'exists</string>\n    <string name="openless_shizuku_permission_rationale">',
  ),
);

const mergedTwice = mergeMissingStringResources(
  mergedAll.content,
  SHIZUKU_STRINGS_BY_LOCALE.values,
);
assert.equal(mergedTwice.changed, false);

for (const [locale, stringsByName] of Object.entries(SHIZUKU_STRINGS_BY_LOCALE)) {
  const localeMerged = mergeMissingStringResources(baseXml, stringsByName);
  assert.equal(localeMerged.changed, true, `locale ${locale} should merge`);
  for (const name of Object.keys(stringsByName)) {
    assert.ok(hasStringResource(localeMerged.content, name), `${locale} missing ${name}`);
  }
}

assert.match(
  formatStringResource('openless_shizuku_open_manager', 'Open Shizuku'),
  /<string name="openless_shizuku_open_manager">Open Shizuku<\/string>/,
);

console.log('copy-android-scaffolding string merge checks passed');

for (const application of [
  '',
  '<application android:usesCleartextTraffic="true" />',
  '<application android:usesCleartextTraffic="true"><activity android:name=".MainActivity" /></application>',
]) {
  const manifest = `<manifest xmlns:android="http://schemas.android.com/apk/res/android">
    <uses-permission android:name="android.permission.INTERNET" />
    ${application}
</manifest>`;
  const merged = mergeKeyboardSettingsDebugActivity(manifest);
  assert.equal(merged.changed, true);
  assert.match(
    merged.content,
    /android:name="com\.openless\.app\.OpenLessKeyboardSettingsActivity"/,
  );
  assert.match(merged.content, /android:exported="false"/);
  assert.match(merged.content, /android.permission.INTERNET/);
  if (application) assert.match(merged.content, /android:usesCleartextTraffic="true"/);
  if (application.includes('MainActivity'))
    assert.match(merged.content, /android:name="\.MainActivity"/);
  assert.equal((merged.content.match(/<application\b/g) ?? []).length, 1);
  assert.equal((merged.content.match(/<\/application>/g) ?? []).length, 1);
  assert.equal(mergeKeyboardSettingsDebugActivity(merged.content).changed, false);
  // Registering a UI test must not enable the Rust-backed Application or services.
  assert.doesNotMatch(merged.content, /OpenLessApplication|OpenLessRuntimeService/);
}
assert.equal(
  mergeKeyboardSettingsDebugActivity(
    '<manifest><application><activity android:name=".OpenLessKeyboardSettingsActivity" /></application></manifest>',
  ).changed,
  false,
);
console.log('keyboard settings debug manifest checks passed');

const fixture = mkdtempSync(join(tmpdir(), 'openless-keyboard-debug-manifest-'));
try {
  mkdirSync(join(fixture, 'scripts'));
  const script = join(fixture, 'scripts/copy-android-scaffolding.mjs');
  copyFileSync(new URL('./copy-android-scaffolding.mjs', import.meta.url), script);
  cpSync(new URL('../android/', import.meta.url), join(fixture, 'android'), { recursive: true });
  // The normal build generates this source before copying the scaffolding.
  writeFileSync(join(fixture, 'android/kotlin/OpenLessBuildInfo.kt'), 'package com.openless.app\n');
  cpSync(
    new URL('../src-tauri/icons/android/', import.meta.url),
    join(fixture, 'src-tauri/icons/android'),
    { recursive: true },
  );
  const app = join(fixture, 'src-tauri/gen/android/app');
  mkdirSync(join(app, 'src/debug'), { recursive: true });
  writeFileSync(join(app, 'build.gradle.kts'), 'android {\n    defaultConfig {\n    }\n}\n');
  const manifestPath = join(app, 'src/debug/AndroidManifest.xml');
  const original =
    '<manifest xmlns:android="http://schemas.android.com/apk/res/android"><application android:usesCleartextTraffic="true" /></manifest>';
  writeFileSync(manifestPath, original);
  const run = (args = []) => {
    const result = spawnSync(process.execPath, [script, ...args], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    return readFileSync(manifestPath, 'utf8');
  };
  assert.equal(run(['--dry-run']), original);
  const first = run();
  assert.match(first, /android:name="com\.openless\.app\.OpenLessKeyboardSettingsActivity"/);
  assert.match(first, /android:usesCleartextTraffic="true"/);
  assert.equal(run(), first, 'repeated scaffolding copies must preserve the debug manifest');
} finally {
  rmSync(fixture, { recursive: true, force: true });
}
console.log('keyboard settings instrumentation scaffolding checks passed');
