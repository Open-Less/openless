#!/usr/bin/env node
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const targetPath = fileURLToPath(
  new URL('../src-tauri/gen/android/app/src/main/AndroidManifest.xml', import.meta.url),
);
const sourcePath = fileURLToPath(
  new URL('../src-tauri/android-scaffolding/AndroidManifest.v1.snippet.xml', import.meta.url),
);

const RECORD_AUDIO_RE = /android\.permission\.RECORD_AUDIO/;
const PERMISSION_LINE_RE =
  /<uses-permission[^>]*android:name="android\.permission\.RECORD_AUDIO"[^>]*\/?>/;

function printHelp() {
  console.log(`Usage: node scripts/merge-android-v1-manifest.mjs [options]

Merge APK v1 RECORD_AUDIO permission from android-scaffolding into the generated
AndroidManifest.xml (post \`tauri android init\`).

Options:
  --dry-run   Print planned changes without writing the manifest
  --help      Show this help text

Target: ${targetPath}
Source: ${sourcePath}
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

function extractRecordAudioPermission(snippetXml) {
  const match = snippetXml.match(PERMISSION_LINE_RE);
  if (!match) {
    throw new Error(
      `Source manifest snippet does not contain RECORD_AUDIO permission: ${sourcePath}`,
    );
  }
  return match[0];
}

function mergeRecordAudioPermission(manifestXml, permissionLine) {
  if (RECORD_AUDIO_RE.test(manifestXml)) {
    return { changed: false, content: manifestXml };
  }

  const applicationIdx = manifestXml.indexOf('<application');
  if (applicationIdx !== -1) {
    const indentMatch = manifestXml.slice(0, applicationIdx).match(/(^|\n)([ \t]*)<[^/][^\n]*$/);
    const indent = indentMatch?.[2] ?? '    ';
    const insertion = `${indent}${permissionLine}\n`;
    return {
      changed: true,
      content: `${manifestXml.slice(0, applicationIdx)}${insertion}${manifestXml.slice(applicationIdx)}`,
    };
  }

  const closingManifestIdx = manifestXml.lastIndexOf('</manifest>');
  if (closingManifestIdx === -1) {
    throw new Error(`Target manifest is missing </manifest>: ${targetPath}`);
  }

  const indent = '    ';
  const insertion = `${indent}${permissionLine}\n`;
  return {
    changed: true,
    content: `${manifestXml.slice(0, closingManifestIdx)}${insertion}${manifestXml.slice(closingManifestIdx)}`,
  };
}

function main() {
  const { dryRun } = parseArgs(process.argv.slice(2));

  if (!existsSync(targetPath)) {
    throw new Error(
      `Generated Android manifest not found: ${targetPath}\nRun "npm run tauri -- android init --ci" first.`,
    );
  }
  if (!existsSync(sourcePath)) {
    throw new Error(`Source manifest snippet not found: ${sourcePath}`);
  }

  const permissionLine = extractRecordAudioPermission(readFileSync(sourcePath, 'utf8'));
  const manifestXml = readFileSync(targetPath, 'utf8');
  const { changed, content } = mergeRecordAudioPermission(manifestXml, permissionLine);

  if (!changed) {
    console.log(`RECORD_AUDIO already present in ${targetPath}; skipping merge.`);
    return;
  }

  if (dryRun) {
    console.log(`[dry-run] Would merge RECORD_AUDIO into ${targetPath}`);
    console.log(`[dry-run] Permission line: ${permissionLine}`);
    return;
  }

  writeFileSync(targetPath, content, 'utf8');
  console.log(`Merged RECORD_AUDIO into ${targetPath}`);
}

try {
  main();
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exit(1);
}
