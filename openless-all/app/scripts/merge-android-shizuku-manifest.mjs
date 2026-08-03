#!/usr/bin/env node
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const targetPath = fileURLToPath(
  new URL('../src-tauri/gen/android/app/src/main/AndroidManifest.xml', import.meta.url),
);

const SHIZUKU_PACKAGE = 'moe.shizuku.privileged.api';

const PROVIDER_SNIPPET = `<provider
            android:name="rikka.shizuku.ShizukuProvider"
            android:authorities="${'${applicationId}'}.shizuku"
            android:enabled="true"
            android:exported="true"
            android:multiprocess="false"
            android:permission="android.permission.INTERACT_ACROSS_USERS_FULL" />`;

const ACTIVITY_SNIPPET = `<activity
            android:name=".ShizukuPermissionActivity"
            android:exported="false"
            android:theme="@android:style/Theme.Translucent.NoTitleBar" />`;

const APPLICATION_SNIPPETS = [PROVIDER_SNIPPET, ACTIVITY_SNIPPET];

const QUERIES_SNIPPET = `<queries>
        <package android:name="${SHIZUKU_PACKAGE}" />
    </queries>`;

function printHelp() {
  console.log(`Usage: node scripts/merge-android-shizuku-manifest.mjs [options]

Merge Shizuku provider, permission activity, and package visibility queries.

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

function snippetExists(manifestXml, marker) {
  return manifestXml.includes(marker);
}

const SHIZUKU_PROVIDER_NAME = 'android:name="rikka.shizuku.ShizukuProvider"';

function findProviderTagBounds(manifestXml, marker) {
  const markerIndex = manifestXml.indexOf(marker);
  if (markerIndex === -1) {
    return null;
  }

  const openIndex = manifestXml.lastIndexOf('<provider', markerIndex);
  if (openIndex === -1) {
    return null;
  }

  const selfClosingIndex = manifestXml.indexOf('/>', markerIndex);
  const closeTagIndex = manifestXml.indexOf('</provider>', markerIndex);
  let endIndex = -1;
  if (selfClosingIndex !== -1 && (closeTagIndex === -1 || selfClosingIndex < closeTagIndex)) {
    endIndex = selfClosingIndex + 2;
  } else if (closeTagIndex !== -1) {
    endIndex = closeTagIndex + '</provider>'.length;
  }
  if (endIndex === -1) {
    return null;
  }

  return { start: openIndex, end: endIndex };
}

function ensureProviderAttribute(tagBody, attributeName, attributeValue) {
  const attributePattern = new RegExp(`android:${attributeName}\\s*=\\s*"[^"]*"`);
  if (attributePattern.test(tagBody)) {
    return tagBody.replace(
      attributePattern,
      `android:${attributeName}="${attributeValue}"`,
    );
  }
  return `${tagBody}\n            android:${attributeName}="${attributeValue}"`;
}

function findOpeningTagEnd(tagText, startIndex = 0) {
  let inQuote = false;
  let quoteChar = '';
  for (let i = startIndex; i < tagText.length; i += 1) {
    const ch = tagText[i];
    if ((ch === '"' || ch === "'") && tagText[i - 1] !== '\\') {
      if (!inQuote) {
        inQuote = true;
        quoteChar = ch;
      } else if (ch === quoteChar) {
        inQuote = false;
      }
    }
    if (ch === '>' && !inQuote) {
      return i + 1;
    }
  }
  return -1;
}

function fixProviderOpeningTag(openingTag) {
  if (!openingTag.startsWith('<provider') || !openingTag.includes(SHIZUKU_PROVIDER_NAME)) {
    return openingTag;
  }

  const trimmed = openingTag.trimEnd();
  const selfClosing = trimmed.endsWith('/>');
  let body = trimmed.slice('<provider'.length);
  body = selfClosing ? body.replace(/\/>\s*$/, '') : body.replace(/>\s*$/, '');
  body = body.trimEnd();

  body = ensureProviderAttribute(body, 'enabled', 'true');
  body = ensureProviderAttribute(body, 'exported', 'true');
  body = ensureProviderAttribute(body, 'multiprocess', 'false');
  body = ensureProviderAttribute(
    body,
    'permission',
    'android.permission.INTERACT_ACROSS_USERS_FULL',
  );
  body = ensureProviderAttribute(body, 'authorities', '${applicationId}.shizuku');

  return selfClosing ? `<provider${body} />` : `<provider${body}>`;
}

function fixShizukuProviderBlock(providerBlock) {
  if (!providerBlock.includes(SHIZUKU_PROVIDER_NAME)) {
    return providerBlock;
  }

  const providerStart = providerBlock.indexOf('<provider');
  const openEnd = findOpeningTagEnd(providerBlock, providerStart);
  if (openEnd === -1) {
    return providerBlock;
  }

  const openingTag = providerBlock.slice(0, openEnd);
  const remainder = providerBlock.slice(openEnd);
  const fixedOpening = fixProviderOpeningTag(openingTag);
  return `${fixedOpening}${remainder}`;
}

function fixProviderMultiprocess(manifestXml) {
  const bounds = findProviderTagBounds(manifestXml, SHIZUKU_PROVIDER_NAME);
  if (!bounds) {
    return manifestXml;
  }

  const originalTag = manifestXml.slice(bounds.start, bounds.end);
  const fixedTag = fixShizukuProviderBlock(originalTag);
  if (fixedTag === originalTag) {
    return manifestXml;
  }
  return `${manifestXml.slice(0, bounds.start)}${fixedTag}${manifestXml.slice(bounds.end)}`;
}

function mergeApplicationChildren(manifestXml) {
  let content = manifestXml;
  let changed = false;
  const closingIdx = content.indexOf('</application>');
  if (closingIdx === -1) {
    throw new Error('Target manifest is missing </application>');
  }

  for (const snippet of APPLICATION_SNIPPETS) {
    const marker = snippet.match(/android:name="([^"]+)"/)?.[1];
    if (!marker || snippetExists(content, marker)) {
      continue;
    }
    content = `${content.slice(0, closingIdx)}        ${snippet}\n${content.slice(closingIdx)}`;
    changed = true;
  }

  return { content, changed };
}

function mergeQueries(manifestXml) {
  if (snippetExists(manifestXml, SHIZUKU_PACKAGE)) {
    return { content: manifestXml, changed: false };
  }
  const manifestOpen = manifestXml.match(/^(\s*<manifest\b[^>]*>)/m);
  if (!manifestOpen) {
    throw new Error('Target manifest is missing <manifest> root element');
  }
  const insertAt = manifestOpen.index + manifestOpen[0].length;
  const content =
    `${manifestXml.slice(0, insertAt)}\n    ${QUERIES_SNIPPET}\n${manifestXml.slice(insertAt)}`;
  return { content, changed: true };
}

export function mergeShizukuManifest(manifestXml) {
  const before = manifestXml;
  let content = fixProviderMultiprocess(manifestXml);
  content = mergeApplicationChildren(content).content;
  content = mergeQueries(content).content;
  return { content, changed: content !== before };
}

function main() {
  const { dryRun } = parseArgs(process.argv.slice(2));

  if (!existsSync(targetPath)) {
    throw new Error(
      `Generated Android manifest not found: ${targetPath}\nRun "npm run tauri -- android init --ci" first.`,
    );
  }

  let content = readFileSync(targetPath, 'utf8');
  const before = content;
  const merged = mergeShizukuManifest(content);
  content = merged.content;

  if (!merged.changed) {
    console.log(`Shizuku manifest entries already present in ${targetPath}; skipping merge.`);
    return;
  }

  if (dryRun) {
    console.log(`[dry-run] Would merge Shizuku manifest entries into ${targetPath}`);
    return;
  }

  writeFileSync(targetPath, content, 'utf8');
  console.log(`Merged Shizuku provider / permission activity / queries into ${targetPath}`);
}

try {
  const isDirectRun = Boolean(
    process.argv[1]?.replace(/\\/g, '/').endsWith('merge-android-shizuku-manifest.mjs'),
  );
  if (isDirectRun) {
    main();
  }
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exit(1);
}
