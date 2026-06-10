#!/usr/bin/env node
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const targetPath = fileURLToPath(
  new URL('../src-tauri/gen/android/app/src/main/AndroidManifest.xml', import.meta.url),
);
const sourcePath = fileURLToPath(
  new URL('../android/manifests/AndroidManifest.v1.snippet.xml', import.meta.url),
);

const PERMISSION_LINE_RE =
  /<uses-permission[^>]*android:name="([^"]+)"[^>]*\/?>/g;

function printHelp() {
  console.log(`Usage: node scripts/merge-android-v1-manifest.mjs [options]

Merge APK v1 permissions from android/manifests into the generated
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

function extractPermissionLines(snippetXml) {
  const lines = [];
  for (const match of snippetXml.matchAll(PERMISSION_LINE_RE)) {
    lines.push({ name: match[1], line: match[0] });
  }
  if (lines.length === 0) {
    throw new Error(
      `Source manifest snippet does not contain any uses-permission entries: ${sourcePath}`,
    );
  }
  return lines;
}

function permissionExists(manifestXml, permissionName) {
  const escaped = permissionName.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  return new RegExp(`<uses-permission[^>]*android:name="${escaped}"[^>]*\\/?>`).test(manifestXml);
}

function mergePermissionLines(manifestXml, permissionLines) {
  const missing = permissionLines.filter((permission) => !permissionExists(manifestXml, permission.name));
  if (missing.length === 0) {
    return { changed: false, content: manifestXml };
  }

  const insertionBlock = (indent) => missing.map((permission) => `${indent}${permission.line}`).join('\n') + '\n';

  const applicationIdx = manifestXml.indexOf('<application');
  if (applicationIdx !== -1) {
    const indentMatch = manifestXml.slice(0, applicationIdx).match(/(^|\n)([ \t]*)<[^/][^\n]*$/);
    const indent = indentMatch?.[2] ?? '    ';
    return {
      changed: true,
      content: `${manifestXml.slice(0, applicationIdx)}${insertionBlock(indent)}${manifestXml.slice(applicationIdx)}`,
    };
  }

  const closingManifestIdx = manifestXml.lastIndexOf('</manifest>');
  if (closingManifestIdx === -1) {
    throw new Error(`Target manifest is missing </manifest>: ${targetPath}`);
  }

  const indent = '    ';
  return {
    changed: true,
    content: `${manifestXml.slice(0, closingManifestIdx)}${insertionBlock(indent)}${manifestXml.slice(closingManifestIdx)}`,
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

  const permissionLines = extractPermissionLines(readFileSync(sourcePath, 'utf8'));
  const manifestXml = readFileSync(targetPath, 'utf8');
  const { changed, content } = mergePermissionLines(manifestXml, permissionLines);

  if (!changed) {
    console.log(`APK v1 permissions already present in ${targetPath}; skipping merge.`);
    return;
  }

  if (dryRun) {
    console.log(`[dry-run] Would merge APK v1 permissions into ${targetPath}`);
    for (const permission of permissionLines) {
      console.log(`[dry-run] Permission line: ${permission.line}`);
    }
    return;
  }

  writeFileSync(targetPath, content, 'utf8');
  console.log(`Merged APK v1 permissions into ${targetPath}`);
}

try {
  main();
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exit(1);
}
