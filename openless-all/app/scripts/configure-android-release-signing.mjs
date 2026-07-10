#!/usr/bin/env node
/**
 * Decode ANDROID_KEYSTORE_* env vars and patch gen/android signing for release APK builds.
 */
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
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
  const storePassword = requireEnv('ANDROID_KEYSTORE_PASSWORD');
  const keyAlias = requireEnv('ANDROID_KEY_ALIAS');
  const keyPassword = requireEnv('ANDROID_KEY_PASSWORD');

  if (!existsSync(gradlePath)) {
    throw new Error(`Gradle file not found: ${gradlePath} (run tauri android init first)`);
  }

  mkdirSync(dirname(keystorePath), { recursive: true });
  writeFileSync(keystorePath, Buffer.from(base64, 'base64'));

  const escapedStore = storePassword.replace(/\\/g, '\\\\').replace(/"/g, '\\"');
  const escapedKey = keyPassword.replace(/\\/g, '\\\\').replace(/"/g, '\\"');
  const escapedAlias = keyAlias.replace(/\\/g, '\\\\').replace(/"/g, '\\"');
  const escapedKeystore = keystorePath.replace(/\\/g, '/');

  let content = readFileSync(gradlePath, 'utf8');
  if (content.includes('openlessRelease')) {
    console.log('Release signing already configured in build.gradle.kts');
    return;
  }

  const signingConfigsBlock = `
    signingConfigs {
        create("openlessRelease") {
            storeFile = file("${escapedKeystore}")
            storePassword = "${escapedStore}"
            keyAlias = "${escapedAlias}"
            keyPassword = "${escapedKey}"
        }
    }`;

  if (!/signingConfigs\s*\{/.test(content)) {
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
    content = content.replace(
      /android\s*\{/,
      `android {${signingConfigsBlock}\n    buildTypes {\n        getByName("release") {\n            signingConfig = signingConfigs.getByName("openlessRelease")\n        }\n    }`,
    );
  }

  writeFileSync(gradlePath, content);
  console.log(`Configured release signing in ${gradlePath}`);
}

main();
