#!/usr/bin/env node
/**
 * Decode ANDROID_KEYSTORE_* env vars and patch gen/android signing for release APK builds.
 */
import { chmodSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const appRoot = fileURLToPath(new URL('..', import.meta.url));
const gradlePath = join(appRoot, 'src-tauri/gen/android/app/build.gradle.kts');
const keystorePath = join(appRoot, 'src-tauri/gen/android/openless-release.keystore');

function requireEnv(name) {
  const value = process.env[name];
  if (!value) {
    throw new Error(`${name} is required for Android release signing`);
  }
  return value;
}

function main() {
  const base64 = requireEnv('ANDROID_KEYSTORE_BASE64');
  requireEnv('ANDROID_KEYSTORE_PASSWORD');
  requireEnv('ANDROID_KEY_ALIAS');
  requireEnv('ANDROID_KEY_PASSWORD');

  if (!existsSync(gradlePath)) {
    throw new Error(`Gradle file not found: ${gradlePath} (run tauri android init first)`);
  }

  mkdirSync(dirname(keystorePath), { recursive: true });
  writeFileSync(keystorePath, Buffer.from(base64, 'base64'), { mode: 0o600 });
  chmodSync(keystorePath, 0o600);

  let content = readFileSync(gradlePath, 'utf8');
  if (!/android\s*\{/.test(content)) {
    throw new Error('Android Gradle configuration block not found');
  }

  const releaseSigningConfig = `create("openlessRelease") {
            storeFile = rootProject.file("openless-release.keystore")
            storePassword = System.getenv("ANDROID_KEYSTORE_PASSWORD")
            keyAlias = System.getenv("ANDROID_KEY_ALIAS")
            keyPassword = System.getenv("ANDROID_KEY_PASSWORD")
        }`;
  const signingConfigsBlock = `
    signingConfigs {
        ${releaseSigningConfig}
    }`;

  // Migrate an existing generated block too, so local rebuilds cannot retain
  // the old password literals in cacheable Kotlin DSL source.
  if (/create\("openlessRelease"\)\s*\{/.test(content)) {
    const block = /create\("openlessRelease"\)\s*\{(?:[^{}"]|"(?:\\.|[^"\\])*")*\}/;
    if (!block.test(content)) throw new Error('Unrecognized existing release signing block');
    content = content.replace(block, () => releaseSigningConfig);
  } else if (/signingConfigs\s*\{/.test(content)) {
    content = content.replace(
      /signingConfigs\s*\{/,
      `signingConfigs {\n        ${releaseSigningConfig}`,
    );
  } else {
    content = content.replace(/android\s*\{/, `android {${signingConfigsBlock}`);
  }

  if (/getByName\("release"\)/.test(content)) {
    if (!/signingConfig\s*=\s*signingConfigs\.getByName\("openlessRelease"\)/.test(content)) {
      content = content.replace(
        /getByName\("release"\)\s*\{/,
        'getByName("release") {\n            signingConfig = signingConfigs.getByName("openlessRelease")',
      );
    }
  } else if (/buildTypes\s*\{/.test(content)) {
    content = content.replace(
      /buildTypes\s*\{/,
      `buildTypes {\n        getByName("release") {\n            signingConfig = signingConfigs.getByName("openlessRelease")\n        }`,
    );
  } else {
    // Create the signing config before looking it up in this later block.
    content += `\nandroid {\n    buildTypes {\n        getByName("release") {\n            signingConfig = signingConfigs.getByName("openlessRelease")\n        }\n    }\n}\n`;
  }

  writeFileSync(gradlePath, content);
  console.log(`Configured release signing in ${gradlePath}`);
}

main();
