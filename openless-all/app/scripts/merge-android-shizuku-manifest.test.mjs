#!/usr/bin/env node
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { mergeShizukuManifest } from './merge-android-shizuku-manifest.mjs';

const manifestScript = fileURLToPath(new URL('./merge-android-shizuku-manifest.mjs', import.meta.url));
const depsScript = fileURLToPath(new URL('./patch-android-shizuku-deps.mjs', import.meta.url));
const ANDROID_NAMESPACE_URI = 'http://schemas.android.com/apk/res/android';

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

const singleQuoteProviderFixture = `<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android">
    <application>
        <provider
            android:name='rikka.shizuku.ShizukuProvider'
            android:enabled='false'
            android:exported='false'
            android:multiprocess='true' />
    </application>
</manifest>`;

const singleQuoteMerged = mergeShizukuManifest(singleQuoteProviderFixture);
assert.equal(singleQuoteMerged.changed, true);
const singleQuoteProviderMatch = singleQuoteMerged.content.match(
  /<provider[\s\S]*?rikka\.shizuku\.ShizukuProvider[\s\S]*?(?:\/>|>)/,
);
assert.ok(singleQuoteProviderMatch, 'Shizuku provider opening tag must exist');
const singleQuoteProviderTag = singleQuoteProviderMatch[0];
assert.equal(
  (singleQuoteProviderTag.match(/android:enabled=/g) || []).length,
  1,
  'android:enabled must appear exactly once',
);
assert.match(singleQuoteProviderTag, /android:enabled="true"/);
assert.match(singleQuoteProviderTag, /android:exported="true"/);
assert.match(singleQuoteProviderTag, /android:multiprocess="false"/);
assertParsableManifest(singleQuoteMerged.content);

const compactManifestFixture =
  `<?xml version="1.0"?><manifest xmlns:android="http://schemas.android.com/apk/res/android"><application></application></manifest>`;

const compactMerged = mergeShizukuManifest(compactManifestFixture);
assert.equal(compactMerged.changed, true);
assert.match(compactMerged.content, /moe\.shizuku\.privileged\.api/);
assert.match(compactMerged.content, /ShizukuPermissionActivity/);
assertParsableManifest(compactMerged.content);

const compactMergedTwice = mergeShizukuManifest(compactMerged.content);
assert.equal(compactMergedTwice.changed, false);

for (const quote of ['"', "'"]) {
  const trailingBackslashFixture = `<?xml version=${quote}1.0${quote}?>
<manifest xmlns:android=${quote}http://schemas.android.com/apk/res/android${quote} data-path=${quote}C:\\${quote} data-special=${quote}> [ ]${quote}>
    <application></application>
</manifest>`;
  const trailingBackslashMerged = mergeShizukuManifest(trailingBackslashFixture);
  assert.equal(trailingBackslashMerged.changed, true, `backslash fixture with ${quote} must merge`);
  assert.match(trailingBackslashMerged.content, /<queries>/);
  assert.match(trailingBackslashMerged.content, /<application>[\s\S]*ShizukuPermissionActivity/);
  assert.equal(mergeShizukuManifest(trailingBackslashMerged.content).changed, false);
}

const aliasNamespaceFixture = `<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:a="http://schemas.android.com/apk/res/android">
    <queries>
        <package a:name="moe.shizuku.privileged.api" />
    </queries>
    <application>
        <provider a:name="rikka.shizuku.ShizukuProvider" a:multiprocess="true" />
        <activity a:name=".ShizukuPermissionActivity" />
    </application>
</manifest>`;

const aliasNamespaceMerged = mergeShizukuManifest(aliasNamespaceFixture);
assert.equal(aliasNamespaceMerged.changed, true);
assert.equal((aliasNamespaceMerged.content.match(/<provider\b/g) || []).length, 1);
assert.equal((aliasNamespaceMerged.content.match(/<activity\b/g) || []).length, 1);
assert.equal((aliasNamespaceMerged.content.match(/<package\b/g) || []).length, 1);
assert.doesNotMatch(aliasNamespaceMerged.content, /android:/);
assert.match(aliasNamespaceMerged.content, /a:multiprocess="false"/);
assert.equal(mergeShizukuManifest(aliasNamespaceMerged.content).changed, false);

const partialAliasNamespaceFixture = `<?xml version='1.0' encoding='utf-8'?>
<manifest xmlns:a='http://schemas.android.com/apk/res/android'>
    <application>
        <provider a:name='rikka.shizuku.ShizukuProvider' a:enabled='false' />
    </application>
</manifest>`;

const partialAliasNamespaceMerged = mergeShizukuManifest(partialAliasNamespaceFixture);
assert.equal((partialAliasNamespaceMerged.content.match(/<provider\b/g) || []).length, 1);
assert.equal((partialAliasNamespaceMerged.content.match(/<activity\b/g) || []).length, 1);
assert.equal((partialAliasNamespaceMerged.content.match(/<package\b/g) || []).length, 1);
assert.match(partialAliasNamespaceMerged.content, /a:enabled="true"/);
assert.match(partialAliasNamespaceMerged.content, /a:name="\.ShizukuPermissionActivity"/);
assert.match(partialAliasNamespaceMerged.content, /<package a:name="moe\.shizuku\.privileged\.api"/);
assert.match(partialAliasNamespaceMerged.content, /a:theme="@android:style\/Theme\.Translucent\.NoTitleBar"/);
assert.doesNotMatch(partialAliasNamespaceMerged.content, /@a:style/);
assert.equal(mergeShizukuManifest(partialAliasNamespaceMerged.content).changed, false);

const missingNamespaceFixture = `<?xml version="1.0" encoding="utf-8"?>
<manifest>
    <application>
        <provider android:name="rikka.shizuku.ShizukuProvider" />
    </application>
</manifest>`;

const missingNamespaceMerged = mergeShizukuManifest(missingNamespaceFixture);
assert.match(
  missingNamespaceMerged.content,
  /<manifest\s+xmlns:android="http:\/\/schemas\.android\.com\/apk\/res\/android">/,
);
assert.match(missingNamespaceMerged.content, /android:name="rikka\.shizuku\.ShizukuProvider"/);
assert.equal(mergeShizukuManifest(missingNamespaceMerged.content).changed, false);

const commentedEntriesFixture = `<?xml version="1.0" encoding="utf-8"?>
<?openless fake="<manifest><package android:name='moe.shizuku.privileged.api' /></manifest>"?>
<!-- <manifest fake="true"><provider android:name="rikka.shizuku.ShizukuProvider" /><activity android:name=".ShizukuPermissionActivity" /><package android:name="moe.shizuku.privileged.api" /></manifest> -->
<!DOCTYPE manifest [<!ELEMENT manifest ANY>]>
<manifest xmlns:android="http://schemas.android.com/apk/res/android">
    <![CDATA[ <provider android:name="rikka.shizuku.ShizukuProvider" /> ]]>
    <application></application>
</manifest>`;

const commentedEntriesMerged = mergeShizukuManifest(commentedEntriesFixture);
assert.equal(commentedEntriesMerged.changed, true);
assert.match(
  commentedEntriesMerged.content,
  /<application>\s*<provider[\s\S]*?android:name="rikka\.shizuku\.ShizukuProvider"/,
  'the real provider must be inserted even when a comment or CDATA mentions it',
);
assert.match(
  commentedEntriesMerged.content,
  /<application>[\s\S]*?<activity[\s\S]*?android:name="\.ShizukuPermissionActivity"/,
  'the real activity must be inserted even when a comment mentions it',
);
assert.match(
  commentedEntriesMerged.content,
  /<queries>\s*<package android:name="moe\.shizuku\.privileged\.api"/,
  'the real package query must be inserted even when a comment mentions it',
);
assert.match(
  commentedEntriesMerged.content,
  /<manifest xmlns:android=[\s\S]*?<queries>/,
  'queries must be inserted below the real root rather than into the comment',
);
assert.equal(mergeShizukuManifest(commentedEntriesMerged.content).changed, false);

const multipleAndroidPrefixesFixture = `<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:a="http://schemas.android.com/apk/res/android" xmlns:android="http://schemas.android.com/apk/res/android">
    <queries><package android:name="moe.shizuku.privileged.api" /></queries>
    <application>
        <provider android:name="rikka.shizuku.ShizukuProvider" android:multiprocess="true" />
        <activity android:name=".ShizukuPermissionActivity" />
    </application>
</manifest>`;

const multipleAndroidPrefixesMerged = mergeShizukuManifest(multipleAndroidPrefixesFixture);
assert.equal((multipleAndroidPrefixesMerged.content.match(/<provider\b/g) || []).length, 1);
assert.equal((multipleAndroidPrefixesMerged.content.match(/<activity\b/g) || []).length, 1);
assert.equal((multipleAndroidPrefixesMerged.content.match(/<package\b/g) || []).length, 1);
assert.match(multipleAndroidPrefixesMerged.content, /android:multiprocess="false"/);
assert.equal(mergeShizukuManifest(multipleAndroidPrefixesMerged.content).changed, false);

const locallyAliasedAndroidFixture = `<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android">
    <queries><package android:name="moe.shizuku.privileged.api" /></queries>
    <application xmlns:a="http://schemas.android.com/apk/res/android">
        <provider a:name="rikka.shizuku.ShizukuProvider" a:multiprocess="true" />
        <activity a:name=".ShizukuPermissionActivity" />
    </application>
</manifest>`;

const locallyAliasedAndroidMerged = mergeShizukuManifest(locallyAliasedAndroidFixture);
assert.equal((locallyAliasedAndroidMerged.content.match(/<provider\b/g) || []).length, 1);
assert.equal((locallyAliasedAndroidMerged.content.match(/<activity\b/g) || []).length, 1);
assert.equal((locallyAliasedAndroidMerged.content.match(/<package\b/g) || []).length, 1);
assert.match(locallyAliasedAndroidMerged.content, /a:multiprocess="false"/);
assert.equal(mergeShizukuManifest(locallyAliasedAndroidMerged.content).changed, false);

const shadowedAndroidFixture = `<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android">
    <application>
        <provider xmlns:android="urn:not-android"
            android:name="rikka.shizuku.ShizukuProvider"
            android:enabled="keep" />
    </application>
</manifest>`;

const shadowedAndroidMerged = mergeShizukuManifest(shadowedAndroidFixture);
assert.equal((shadowedAndroidMerged.content.match(/<provider\b/g) || []).length, 2);
assert.match(
  shadowedAndroidMerged.content,
  /xmlns:android="urn:not-android"[\s\S]*android:enabled="keep"/,
  'shadowed non-Android provider must remain untouched',
);
assert.match(
  shadowedAndroidMerged.content,
  /<provider\s+android:name="rikka\.shizuku\.ShizukuProvider"[\s\S]*android:multiprocess="false"/,
  'a real Android namespace Shizuku provider must be added',
);
assert.equal(mergeShizukuManifest(shadowedAndroidMerged.content).changed, false);

const regexPrefixFixture = `<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:a.b="http://schemas.android.com/apk/res/android" xmlns:axb="urn:other">
    <application>
        <provider a.b:name="rikka.shizuku.ShizukuProvider" axb:enabled="keep" />
    </application>
</manifest>`;

const regexPrefixMerged = mergeShizukuManifest(regexPrefixFixture);
assert.match(regexPrefixMerged.content, /axb:enabled="keep"/, 'non-Android axb attribute must remain unchanged');
assert.match(regexPrefixMerged.content, /a\.b:enabled="true"/);
assert.equal((regexPrefixMerged.content.match(/a\.b:enabled=/g) || []).length, 1);
assert.equal(mergeShizukuManifest(regexPrefixMerged.content).changed, false);

for (const [label, rootPrefix, applicationDeclaration] of [
  ['root alias shadowed by application', 'a', 'xmlns:a="urn:not-android"'],
  ['root android shadowed by application', 'android', 'xmlns:android="urn:not-android"'],
]) {
  const applicationShadowFixture = `<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:${rootPrefix}="${ANDROID_NAMESPACE_URI}">
    <application ${applicationDeclaration}></application>
</manifest>`;
  const applicationShadowMerged = mergeShizukuManifest(applicationShadowFixture);
  assert.equal(applicationShadowMerged.changed, true, label);
  assert.equal((applicationShadowMerged.content.match(/<provider\b/g) || []).length, 1, `${label}: one provider`);
  assert.equal((applicationShadowMerged.content.match(/<activity\b/g) || []).length, 1, `${label}: one activity`);
  assert.match(
    applicationShadowMerged.content,
    /<provider\s+xmlns:openlessAndroid="http:\/\/schemas\.android\.com\/apk\/res\/android"[\s\S]*openlessAndroid:name="rikka\.shizuku\.ShizukuProvider"/,
    `${label}: inserted provider must bind its name to the Android URI`,
  );
  assert.match(
    applicationShadowMerged.content,
    /<activity\s+xmlns:openlessAndroid="http:\/\/schemas\.android\.com\/apk\/res\/android"[\s\S]*openlessAndroid:name="\.ShizukuPermissionActivity"/,
    `${label}: inserted activity must bind its name to the Android URI`,
  );
  assert.equal(mergeShizukuManifest(applicationShadowMerged.content).changed, false, `${label}: idempotent`);
}

const unprefixedNameFixture = `<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="${ANDROID_NAMESPACE_URI}">
    <queries><package name="moe.shizuku.privileged.api" /></queries>
    <application>
        <provider name="rikka.shizuku.ShizukuProvider" />
        <activity name=".ShizukuPermissionActivity" />
    </application>
</manifest>`;
const unprefixedNameMerged = mergeShizukuManifest(unprefixedNameFixture);
assert.equal((unprefixedNameMerged.content.match(/<provider\b/g) || []).length, 2, 'plain name must not count as android:name');
assert.equal((unprefixedNameMerged.content.match(/<activity\b/g) || []).length, 2, 'plain name must not count as android:name');
assert.equal((unprefixedNameMerged.content.match(/<package\b/g) || []).length, 2, 'plain name must not count as android:name');
assert.match(unprefixedNameMerged.content, /<provider\s+android:name="rikka\.shizuku\.ShizukuProvider"/);
assert.match(unprefixedNameMerged.content, /<package android:name="moe\.shizuku\.privileged\.api"/);
assert.equal(mergeShizukuManifest(unprefixedNameMerged.content).changed, false);

const foreignProviderFixture = `<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="${ANDROID_NAMESPACE_URI}" xmlns:x="urn:foreign">
    <application>
        <x:provider android:name="rikka.shizuku.ShizukuProvider" x:enabled="keep" />
    </application>
</manifest>`;
const foreignProviderMerged = mergeShizukuManifest(foreignProviderFixture);
assert.match(foreignProviderMerged.content, /<x:provider android:name="rikka\.shizuku\.ShizukuProvider" x:enabled="keep"\s*\/>/, 'foreign provider must remain untouched');
assert.match(foreignProviderMerged.content, /<provider\s+android:name="rikka\.shizuku\.ShizukuProvider"/);
assert.equal((foreignProviderMerged.content.match(/<provider\b/g) || []).length, 1, 'one real provider must be injected');
assert.equal(mergeShizukuManifest(foreignProviderMerged.content).changed, false);

console.log('Shizuku Android scaffolding contract checks passed');
