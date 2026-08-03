#!/usr/bin/env node
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { mergeShizukuManifest } from './merge-android-shizuku-manifest.mjs';

const manifestScript = fileURLToPath(new URL('./merge-android-shizuku-manifest.mjs', import.meta.url));
const depsScript = fileURLToPath(new URL('./patch-android-shizuku-deps.mjs', import.meta.url));

const manifestSource = readFileSync(manifestScript, 'utf8');
const depsSource = readFileSync(depsScript, 'utf8');

assert.match(manifestSource, /android:multiprocess="false"/, 'Shizuku provider must set multiprocess=false');
assert.match(
  manifestSource,
  /moe\.shizuku\.privileged\.api/,
  'Shizuku package visibility query must be declared',
);
assert.match(
  manifestSource,
  /fixProviderMultiprocess/,
  'merge script must upgrade legacy multiprocess=true manifests',
);

assert.match(depsSource, /dev\.rikka\.shizuku:api:13\.1\.5/, 'Shizuku API dependency must be pinned');
assert.match(depsSource, /dev\.rikka\.shizuku:provider:13\.1\.5/, 'Shizuku provider dependency must be pinned');
assert.match(depsSource, /aidl = true/, 'Gradle patch must enable AIDL build feature');

const fixture = `<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android">
    <application>
        <activity android:name=".MainActivity" />
        <provider
            android:name="rikka.shizuku.ShizukuProvider"
            android:multiprocess="true" />
    </application>
</manifest>`;

const mergedOnce = mergeShizukuManifest(fixture);
assert.equal(mergedOnce.changed, true);
assert.match(mergedOnce.content, /android:multiprocess="false"/);
assert.match(mergedOnce.content, /ShizukuPermissionActivity/);
assert.match(mergedOnce.content, /moe\.shizuku\.privileged\.api/);

const mergedTwice = mergeShizukuManifest(mergedOnce.content);
assert.equal(mergedTwice.changed, false);

const crossProviderFixture = `<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android">
    <application>
        <provider
            android:name="rikka.shizuku.ShizukuProvider"
            android:authorities="\${applicationId}.shizuku" />
        <provider
            android:name="com.example.OtherProvider"
            android:multiprocess="true" />
    </application>
</manifest>`;

const crossProviderMerged = mergeShizukuManifest(crossProviderFixture);
assert.equal(crossProviderMerged.changed, true);
assert.match(
  crossProviderMerged.content,
  /android:name="rikka\.shizuku\.ShizukuProvider"[\s\S]*android:multiprocess="false"/,
);
assert.match(
  crossProviderMerged.content,
  /android:name="com\.example\.OtherProvider"[\s\S]*android:multiprocess="true"/,
);

function assertParsableManifest(xml) {
  const malformedPatterns = [
    /<meta-data[^>]*\n\s+android:(enabled|exported|multiprocess|authorities|permission)=/,
    /<meta-data[^>]*\/\s+android:/,
  ];
  for (const pattern of malformedPatterns) {
    assert.doesNotMatch(xml, pattern, `malformed manifest fragment: ${pattern}`);
  }

  const tagPattern = /<\/?([A-Za-z][\w:.-]*)([^>]*)>/g;
  const stack = [];
  let match = tagPattern.exec(xml);
  while (match !== null) {
    const [full, name] = match;
    if (full.startsWith('<?') || full.startsWith('<!')) {
      match = tagPattern.exec(xml);
      continue;
    }
    if (full.startsWith('</')) {
      assert.ok(stack.length > 0, `unexpected closing tag ${name}`);
      assert.equal(stack.pop(), name, `mismatched closing tag ${name}`);
      match = tagPattern.exec(xml);
      continue;
    }
    if (match[2].trim().endsWith('/')) {
      match = tagPattern.exec(xml);
      continue;
    }
    stack.push(name);
    match = tagPattern.exec(xml);
  }
  assert.equal(stack.length, 0, `unclosed tags remain: ${stack.join(', ')}`);
}

const pairedProviderFixture = `<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android">
    <application>
        <provider
            android:name="rikka.shizuku.ShizukuProvider"
            android:multiprocess="true">
            <meta-data android:name="x" android:value="y" />
        </provider>
        <provider
            android:name="com.example.OtherProvider"
            android:multiprocess="true" />
    </application>
</manifest>`;

const pairedProviderMerged = mergeShizukuManifest(pairedProviderFixture);
assert.equal(pairedProviderMerged.changed, true);
assert.match(
  pairedProviderMerged.content,
  /<meta-data android:name="x" android:value="y"\s*\/>/,
  'meta-data child must remain intact',
);
assert.match(
  pairedProviderMerged.content,
  /<provider[\s\S]*android:name="rikka\.shizuku\.ShizukuProvider"[\s\S]*android:multiprocess="false"[\s\S]*>\s*<meta-data/,
  'Shizuku provider opening tag must be fixed before children',
);
assert.match(
  pairedProviderMerged.content,
  /android:name="com\.example\.OtherProvider"[\s\S]*android:multiprocess="true"/,
);
assertParsableManifest(pairedProviderMerged.content);

const multiChildProviderFixture = `<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android">
    <application>
        <provider android:name="rikka.shizuku.ShizukuProvider">
            <meta-data android:name="a" android:value="1" />
            <meta-data android:name="b" android:value="2" />
        </provider>
    </application>
</manifest>`;

const multiChildMerged = mergeShizukuManifest(multiChildProviderFixture);
assert.equal(multiChildMerged.changed, true);
assert.match(multiChildMerged.content, /<meta-data android:name="a" android:value="1"\s*\/>/);
assert.match(multiChildMerged.content, /<meta-data android:name="b" android:value="2"\s*\/>/);
assertParsableManifest(multiChildMerged.content);

assertParsableManifest(mergedOnce.content);
assertParsableManifest(crossProviderMerged.content);

const wrongProviderAttributesFixture = `<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android">
    <application>
        <provider
            android:name="rikka.shizuku.ShizukuProvider"
            android:authorities="wrong.authority"
            android:enabled="false"
            android:exported="false"
            android:multiprocess="true"
            android:permission="com.example.WRONG" />
        <provider
            android:name="com.example.OtherProvider"
            android:enabled="false"
            android:exported="false"
            android:multiprocess="true"
            android:permission="com.example.KEEP" />
    </application>
</manifest>`;

const wrongProviderMerged = mergeShizukuManifest(wrongProviderAttributesFixture);
assert.equal(wrongProviderMerged.changed, true);
const shizukuProviderMatch = wrongProviderMerged.content.match(
  /<provider[\s\S]*?android:name="rikka\.shizuku\.ShizukuProvider"[\s\S]*?(?:\/>|>)/,
);
assert.ok(shizukuProviderMatch, 'Shizuku provider opening tag must exist');
const shizukuProviderTag = shizukuProviderMatch[0];
assert.match(shizukuProviderTag, /android:authorities="\$\{applicationId\}\.shizuku"/);
assert.match(shizukuProviderTag, /android:enabled="true"/);
assert.match(shizukuProviderTag, /android:exported="true"/);
assert.match(shizukuProviderTag, /android:multiprocess="false"/);
assert.match(
  shizukuProviderTag,
  /android:permission="android\.permission\.INTERACT_ACROSS_USERS_FULL"/,
);
assert.doesNotMatch(shizukuProviderTag, /wrong\.authority/);
assert.match(
  wrongProviderMerged.content,
  /android:name="com\.example\.OtherProvider"[\s\S]*android:enabled="false"[\s\S]*android:exported="false"[\s\S]*android:multiprocess="true"[\s\S]*android:permission="com\.example\.KEEP"/,
);
assertParsableManifest(wrongProviderMerged.content);

console.log('Shizuku Android scaffolding contract checks passed');
