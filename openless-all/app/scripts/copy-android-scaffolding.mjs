#!/usr/bin/env node
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const appRoot = fileURLToPath(new URL('..', import.meta.url));
const kotlinRoot = join(appRoot, 'android/kotlin');
const kotlinTestRoot = join(kotlinRoot, 'test');
const kotlinAndroidTestRoot = join(kotlinRoot, 'androidTest');
const manifestsRoot = join(appRoot, 'android/manifests');
const androidIconRoot = join(appRoot, 'src-tauri/icons/android');
const androidAppRoot = join(appRoot, 'src-tauri/gen/android/app');
const genRoot = join(appRoot, 'src-tauri/gen/android/app/src/main');
const kotlinDest = join(genRoot, 'java/com/openless/app');
const kotlinTestDest = join(androidAppRoot, 'src/test/java/com/openless/app');
const kotlinAndroidTestDest = join(androidAppRoot, 'src/androidTest/java/com/openless/app');
const androidAppGradle = join(androidAppRoot, 'build.gradle.kts');
const resDest = join(genRoot, 'res');
const resXmlDest = join(genRoot, 'res/xml');

const KOTLIN_FILES = [
  'OpenLessAppContext.kt',
  'OpenLessNative.kt',
  'OpenLessPermissionBridge.kt',
  'MicrophonePermissionActivity.kt',
  'OpenLessAndroidPreferences.kt',
  'OpenLessCredentialCipher.kt',
  'OpenLessCredentialVault.kt',
  'OpenLessApplication.kt',
  'OpenLessOverlayService.kt',
  'OpenLessOverlayBridge.kt',
  'OpenLessAccessibilityService.kt',
  'OpenLessAccessibilityCommandReceiver.kt',
  'OverlayPermissionActivity.kt',
  'OpenLessUpdateInstaller.kt',
];

const KOTLIN_TEST_FILES = ['OpenLessCredentialCipherTest.kt'];
const KOTLIN_ANDROID_TEST_FILES = ['OpenLessCredentialVaultInstrumentedTest.kt'];

const XML_FILES = [
  ['res/xml/openless_accessibility_config.xml', 'openless_accessibility_config.xml'],
];

const GENERATED_ACCESSIBILITY_CONFIG = `<?xml version="1.0" encoding="utf-8"?>
<accessibility-service xmlns:android="http://schemas.android.com/apk/res/android"
    android:accessibilityEventTypes="typeWindowStateChanged|typeWindowsChanged|typeViewFocused"
    android:accessibilityFeedbackType="feedbackGeneric"
    android:accessibilityFlags="flagRetrieveInteractiveWindows"
    android:canRetrieveWindowContent="true"
    android:description="@string/openless_accessibility_description"
    android:notificationTimeout="100"
    android:settingsActivity="com.openless.app.MainActivity" />
`;

const GENERATED_STRINGS_SNIPPET = `
    <string name="openless_accessibility_description">OpenLess uses accessibility to detect the keyboard and paste dictation results without switching your current keyboard.</string>
`;

function printHelp() {
  console.log(`Usage: node scripts/copy-android-scaffolding.mjs [options]

Copy Kotlin scaffolding and XML resources into gen/android after \`tauri android init\`.

Options:
  --dry-run   Print planned copies without writing
  --help      Show this help text
`);
}

function parseArgs(argv) {
  let dryRun = false;
  for (const arg of argv) {
    if (arg === '--help' || arg === '-h') {
      printHelp();
      process.exit(0);
    }
    if (arg === '--dry-run') {
      dryRun = true;
      continue;
    }
    throw new Error(`Unknown argument: ${arg}`);
  }
  return { dryRun };
}

function ensureDir(path, dryRun) {
  if (dryRun || existsSync(path)) {
    return;
  }
  mkdirSync(path, { recursive: true });
}

function mergeStringsXml(dryRun) {
  const stringsPath = join(genRoot, 'res/values/strings.xml');
  if (!existsSync(stringsPath)) {
    const content = `<?xml version="1.0" encoding="utf-8"?>
<resources>${GENERATED_STRINGS_SNIPPET}
</resources>
`;
    if (dryRun) {
      console.log(`[dry-run] Would create ${stringsPath}`);
      return;
    }
    ensureDir(dirname(stringsPath), dryRun);
    writeFileSync(stringsPath, content, 'utf8');
    console.log(`Created ${stringsPath}`);
    return;
  }

  const existing = readFileSync(stringsPath, 'utf8');
  if (existing.includes('openless_accessibility_description')) {
    console.log(`OpenLess strings already present in ${stringsPath}; skipping.`);
    return;
  }

  const updated = existing.replace('</resources>', `${GENERATED_STRINGS_SNIPPET}\n</resources>`);
  if (dryRun) {
    console.log(`[dry-run] Would merge OpenLess strings into ${stringsPath}`);
    return;
  }
  writeFileSync(stringsPath, updated, 'utf8');
  console.log(`Merged OpenLess strings into ${stringsPath}`);
}

function copyDirectoryContents(srcRoot, destRoot, dryRun) {
  if (!existsSync(srcRoot)) {
    throw new Error(`Missing Android icon resources: ${srcRoot}`);
  }

  ensureDir(destRoot, dryRun);
  for (const entry of readdirSync(srcRoot)) {
    const src = join(srcRoot, entry);
    const dest = join(destRoot, entry);
    if (statSync(src).isDirectory()) {
      copyDirectoryContents(src, dest, dryRun);
      continue;
    }
    if (dryRun) {
      console.log(`[dry-run] Would copy ${src} -> ${dest}`);
      continue;
    }
    ensureDir(dirname(dest), dryRun);
    copyFileSync(src, dest);
    console.log(`Copied ${dest}`);
  }
}

function copyNamedFiles(files, srcRoot, destRoot, dryRun) {
  ensureDir(destRoot, dryRun);
  for (const file of files) {
    const src = join(srcRoot, file);
    const dest = join(destRoot, file);
    if (!existsSync(src)) {
      throw new Error(`Missing scaffolding file: ${src}`);
    }
    if (dryRun) {
      console.log(`[dry-run] Would copy ${src} -> ${dest}`);
      continue;
    }
    copyFileSync(src, dest);
    console.log(`Copied ${file}`);
  }
}

function ensureInstrumentationRunner(dryRun) {
  if (!existsSync(androidAppGradle)) {
    throw new Error(`Missing generated Android Gradle file: ${androidAppGradle}`);
  }
  const existing = readFileSync(androidAppGradle, 'utf8');
  if (existing.includes('testInstrumentationRunner')) {
    console.log(`Android instrumentation runner already present in ${androidAppGradle}; skipping.`);
    return;
  }
  const defaultConfig = /^(\s*)defaultConfig\s*\{\s*$/m;
  const match = existing.match(defaultConfig);
  if (!match) {
    throw new Error(`defaultConfig block not found in ${androidAppGradle}`);
  }
  const runner =
    `${match[0]}\n${match[1]}    ` +
    'testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"';
  const updated = existing.replace(defaultConfig, runner);
  if (dryRun) {
    console.log(`[dry-run] Would add testInstrumentationRunner to ${androidAppGradle}`);
    return;
  }
  writeFileSync(androidAppGradle, updated, 'utf8');
  console.log(`Added Android instrumentation runner to ${androidAppGradle}`);
}

function main() {
  const { dryRun } = parseArgs(process.argv.slice(2));

  if (!existsSync(join(appRoot, 'src-tauri/gen/android'))) {
    throw new Error(
      `Generated Android project not found under src-tauri/gen/android.\nRun "npm run tauri -- android init --ci" first.`,
    );
  }

  ensureDir(kotlinDest, dryRun);
  ensureDir(kotlinTestDest, dryRun);
  ensureDir(kotlinAndroidTestDest, dryRun);
  ensureDir(resXmlDest, dryRun);
  copyDirectoryContents(androidIconRoot, resDest, dryRun);
  copyNamedFiles(KOTLIN_FILES, kotlinRoot, kotlinDest, dryRun);
  copyNamedFiles(KOTLIN_TEST_FILES, kotlinTestRoot, kotlinTestDest, dryRun);
  copyNamedFiles(
    KOTLIN_ANDROID_TEST_FILES,
    kotlinAndroidTestRoot,
    kotlinAndroidTestDest,
    dryRun,
  );
  ensureInstrumentationRunner(dryRun);

  for (const [relSrc, destName] of XML_FILES) {
    const src = join(manifestsRoot, relSrc);
    const dest = join(resXmlDest, destName);
    const content = existsSync(src)
      ? readFileSync(src, 'utf8')
      : GENERATED_ACCESSIBILITY_CONFIG;
    if (dryRun) {
      console.log(`[dry-run] Would write ${dest}`);
      continue;
    }
    writeFileSync(dest, content, 'utf8');
    console.log(`Wrote ${destName}`);
  }

  mergeStringsXml(dryRun);
}

try {
  main();
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exit(1);
}
