#!/usr/bin/env node
import assert from 'node:assert/strict';
import {
  AIDL_FEATURE,
  patchAidlBuildFeature,
  patchShizukuDependencies,
  patchShizukuGradle,
  SHIZUKU_API,
} from './patch-android-shizuku-deps.mjs';

const template = `
plugins {
    id("com.android.application")
}

android {
    namespace = "com.openless.app"
    buildFeatures {
        buildConfig = true
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.0.0")
}
`;

const patchedOnce = patchShizukuGradle(template);
assert.equal(patchedOnce.changed, true);
assert.ok(patchedOnce.content.includes(SHIZUKU_API));
assert.ok(patchedOnce.content.includes(AIDL_FEATURE));
assert.match(patchedOnce.content, /buildFeatures\s*\{[^}]*buildConfig = true[^}]*aidl = true/s);

const patchedTwice = patchShizukuGradle(patchedOnce.content);
assert.equal(patchedTwice.changed, false);

const noBuildFeatures = `
android {
    namespace = "com.openless.app"
}
dependencies {
}
`;
const inserted = patchAidlBuildFeature(noBuildFeatures);
assert.equal(inserted.changed, true);
assert.ok(inserted.content.includes(AIDL_FEATURE));

const depsOnly = patchShizukuDependencies('dependencies {\n}\n');
assert.equal(depsOnly.changed, true);
assert.ok(depsOnly.content.includes(SHIZUKU_API));

const aidlFalseTemplate = `
android {
    buildFeatures {
        buildConfig = true
        aidl = false
    }
}
`;
const aidlFalsePatched = patchAidlBuildFeature(aidlFalseTemplate);
assert.equal(aidlFalsePatched.changed, true);
assert.ok(aidlFalsePatched.content.includes(AIDL_FEATURE));
assert.ok(!/aidl\s*=\s*false/.test(aidlFalsePatched.content));

console.log('patch-android-shizuku-deps contract checks passed');
