#!/usr/bin/env node
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const targetPath = fileURLToPath(
  new URL('../src-tauri/gen/android/app/src/main/AndroidManifest.xml', import.meta.url),
);

const PERMISSIONS = [
  'android.permission.SYSTEM_ALERT_WINDOW',
  'android.permission.FOREGROUND_SERVICE',
  'android.permission.FOREGROUND_SERVICE_MICROPHONE',
];

const APPLICATION_SNIPPET = `
        <application android:name=".OpenLessApplication">
        </application>
`;

const SERVICE_SNIPPETS = [
  `<service
            android:name=".OpenLessOverlayService"
            android:exported="false"
            android:foregroundServiceType="microphone" />`,
  `<service
            android:name=".OpenLessAccessibilityService"
            android:process=":accessibility"
            android:exported="false"
            android:permission="android.permission.BIND_ACCESSIBILITY_SERVICE">
            <intent-filter>
                <action android:name="android.accessibilityservice.AccessibilityService" />
            </intent-filter>
            <meta-data
                android:name="android.accessibilityservice"
                android:resource="@xml/openless_accessibility_config" />
        </service>`,
  `<receiver
            android:name=".OpenLessAccessibilityCommandReceiver"
            android:process=":accessibility"
            android:exported="false" />`,
  `<activity
            android:name=".OverlayPermissionActivity"
            android:exported="false"
            android:theme="@android:style/Theme.Translucent.NoTitleBar" />`,
  `<activity
            android:name=".MicrophonePermissionActivity"
            android:exported="false"
            android:theme="@android:style/Theme.Translucent.NoTitleBar" />`,
];

function printHelp() {
  console.log(`Usage: node scripts/merge-android-overlay-manifest.mjs [options]

Merge overlay / accessibility declarations into generated AndroidManifest.xml.

Options:
  --dry-run   Print planned changes without writing
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

function permissionExists(manifestXml, permissionName) {
  const escaped = permissionName.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  return new RegExp(`<uses-permission[^>]*android:name="${escaped}"[^>]*\\/?>`).test(manifestXml);
}

function mergePermissions(manifestXml) {
  let content = manifestXml;
  let changed = false;
  for (const name of PERMISSIONS) {
    if (permissionExists(content, name)) {
      continue;
    }
    const line = `    <uses-permission android:name="${name}" />`;
    const applicationIdx = content.indexOf('<application');
    if (applicationIdx === -1) {
      throw new Error('Target manifest is missing <application>');
    }
    content = `${content.slice(0, applicationIdx)}${line}\n${content.slice(applicationIdx)}`;
    changed = true;
  }
  return { content, changed };
}

function ensureApplicationName(manifestXml) {
  if (/android:name="\.OpenLessApplication"/.test(manifestXml)) {
    return { content: manifestXml, changed: false };
  }
  const updated = manifestXml.replace(
    /<application(\s|>)/,
    '<application android:name=".OpenLessApplication"$1',
  );
  return { content: updated, changed: updated !== manifestXml };
}

function snippetExists(manifestXml, marker) {
  return manifestXml.includes(marker);
}

function mergeApplicationChildren(manifestXml) {
  let content = manifestXml;
  let changed = false;
  const closingIdx = content.indexOf('</application>');
  if (closingIdx === -1) {
    throw new Error('Target manifest is missing </application>');
  }

  for (const snippet of SERVICE_SNIPPETS) {
    const marker = snippet.match(/android:name="([^"]+)"/)?.[1];
    if (!marker || snippetExists(content, marker)) {
      continue;
    }
    content = `${content.slice(0, closingIdx)}        ${snippet}\n${content.slice(closingIdx)}`;
    changed = true;
  }

  return { content, changed };
}

function main() {
  const { dryRun } = parseArgs(process.argv.slice(2));

  if (!existsSync(targetPath)) {
    throw new Error(
      `Generated Android manifest not found: ${targetPath}\nRun "npm run tauri -- android init --ci" first.`,
    );
  }

  let content = readFileSync(targetPath, 'utf8');
  let changed = false;

  for (const step of [mergePermissions, ensureApplicationName, mergeApplicationChildren]) {
    const result = step(content);
    content = result.content;
    changed = changed || result.changed;
  }

  if (!changed) {
    console.log(`Overlay manifest entries already present in ${targetPath}; skipping merge.`);
    return;
  }

  if (dryRun) {
    console.log(`[dry-run] Would merge overlay manifest entries into ${targetPath}`);
    return;
  }

  writeFileSync(targetPath, content, 'utf8');
  console.log(`Merged overlay / accessibility entries into ${targetPath}`);
}

try {
  main();
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exit(1);
}
