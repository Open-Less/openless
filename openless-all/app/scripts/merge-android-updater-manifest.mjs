#!/usr/bin/env node
/**
 * Merge REQUEST_INSTALL_PACKAGES + FileProvider for in-app APK updates.
 */
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const targetPath = fileURLToPath(
  new URL('../src-tauri/gen/android/app/src/main/AndroidManifest.xml', import.meta.url),
);
const resXmlPath = fileURLToPath(
  new URL('../src-tauri/gen/android/app/src/main/res/xml/openless_file_paths.xml', import.meta.url),
);

const PERMISSIONS = [
  'android.permission.REQUEST_INSTALL_PACKAGES',
];

const PROVIDER_SNIPPET = `
        <provider
            android:name="androidx.core.content.FileProvider"
            android:authorities="\${applicationId}.fileprovider"
            android:exported="false"
            android:grantUriPermissions="true">
            <meta-data
                android:name="android.support.FILE_PROVIDER_PATHS"
                android:resource="@xml/openless_file_paths" />
        </provider>`;

const FILE_PATHS_XML = `<?xml version="1.0" encoding="utf-8"?>
<paths xmlns:android="http://schemas.android.com/apk/res/android">
    <cache-path name="updates" path="updates/" />
    <external-cache-path name="external_updates" path="updates/" />
</paths>
`;

function permissionExists(manifestXml, permissionName) {
  const escaped = permissionName.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  return new RegExp(`<uses-permission[^>]*android:name="${escaped}"[^>]*\\/?>`).test(manifestXml);
}

function providerExists(manifestXml) {
  return /androidx\.core\.content\.FileProvider/.test(manifestXml);
}

function mergePermissions(manifestXml) {
  let content = manifestXml;
  let changed = false;
  for (const name of PERMISSIONS) {
    if (permissionExists(content, name)) continue;
    const line = `    <uses-permission android:name="${name}" />`;
    const applicationIdx = content.indexOf('<application');
    if (applicationIdx === -1) throw new Error('Target manifest is missing <application>');
    content = `${content.slice(0, applicationIdx)}${line}\n${content.slice(applicationIdx)}`;
    changed = true;
  }
  return { content, changed };
}

function mergeProvider(manifestXml) {
  if (providerExists(manifestXml)) {
    return { content: manifestXml, changed: false };
  }
  const closeTag = '</application>';
  const idx = manifestXml.lastIndexOf(closeTag);
  if (idx === -1) throw new Error('Target manifest is missing </application>');
  const content = `${manifestXml.slice(0, idx)}${PROVIDER_SNIPPET}\n    ${manifestXml.slice(idx)}`;
  return { content, changed: true };
}

function main() {
  if (!existsSync(targetPath)) {
    throw new Error(`Target manifest not found: ${targetPath}`);
  }
  let content = readFileSync(targetPath, 'utf8');
  const perm = mergePermissions(content);
  content = perm.content;
  const prov = mergeProvider(content);
  content = prov.content;
  if (perm.changed || prov.changed) {
    writeFileSync(targetPath, content);
    console.log(`Updated ${targetPath}`);
  } else {
    console.log(`No manifest changes needed: ${targetPath}`);
  }

  const resDir = resXmlPath.replace(/[/\\][^/\\]+$/, '');
  if (!existsSync(resDir)) {
    mkdirSync(resDir, { recursive: true });
  }
  if (!existsSync(resXmlPath)) {
    writeFileSync(resXmlPath, FILE_PATHS_XML);
    console.log(`Wrote ${resXmlPath}`);
  }
}

main();
