#!/usr/bin/env node
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const targetPath = fileURLToPath(
  new URL('../src-tauri/gen/android/app/src/main/AndroidManifest.xml', import.meta.url),
);

const SHIZUKU_PACKAGE = 'moe.shizuku.privileged.api';
const SHIZUKU_PROVIDER_CLASS = 'rikka.shizuku.ShizukuProvider';
const ANDROID_NAMESPACE_URI = 'http://schemas.android.com/apk/res/android';

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

const APPLICATION_SNIPPETS = [
  { tagName: 'provider', name: SHIZUKU_PROVIDER_CLASS, snippet: PROVIDER_SNIPPET },
  { tagName: 'activity', name: '.ShizukuPermissionActivity', snippet: ACTIVITY_SNIPPET },
];

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

function findXmlMarkupEnd(xml, startIndex) {
  let inQuote = false;
  let quoteChar = '';
  let subsetDepth = 0;
  for (let index = startIndex; index < xml.length; index += 1) {
    const ch = xml[index];
    if (ch === '"' || ch === "'") {
      if (!inQuote) {
        inQuote = true;
        quoteChar = ch;
      } else if (ch === quoteChar) {
        inQuote = false;
      }
      continue;
    }
    if (inQuote) continue;
    if (ch === '[') subsetDepth += 1;
    if (ch === ']' && subsetDepth > 0) subsetDepth -= 1;
    if (ch === '>' && subsetDepth === 0) return index + 1;
  }
  return -1;
}

/**
 * Enumerates real XML tags only. Comments, CDATA, processing instructions, and
 * declarations are skipped so their text cannot satisfy manifest checks.
 */
function scanXmlTags(xml) {
  const tags = [];
  const namespaceStack = [];
  let cursor = 0;
  while (cursor < xml.length) {
    const start = xml.indexOf('<', cursor);
    if (start === -1) break;
    if (xml.startsWith('<!--', start)) {
      const end = xml.indexOf('-->', start + 4);
      cursor = end === -1 ? xml.length : end + 3;
      continue;
    }
    if (xml.startsWith('<![CDATA[', start)) {
      const end = xml.indexOf(']]>', start + 9);
      cursor = end === -1 ? xml.length : end + 3;
      continue;
    }
    if (xml.startsWith('<?', start)) {
      const end = xml.indexOf('?>', start + 2);
      cursor = end === -1 ? xml.length : end + 2;
      continue;
    }
    if (xml.startsWith('<!', start)) {
      const end = findXmlMarkupEnd(xml, start + 2);
      cursor = end === -1 ? xml.length : end;
      continue;
    }

    const end = findOpeningTagEnd(xml, start);
    if (end === -1) break;
    const tag = xml.slice(start, end);
    const nameMatch = tag.match(/^<\/?\s*([A-Za-z_][\w:.-]*)\b/);
    if (nameMatch) {
      const closing = /^<\//.test(tag);
      const name = nameMatch[1];
      const parentScope = namespaceStack.at(-1)?.scope ?? new Map();
      const scope = new Map(parentScope);
      const attributes = parseAttributes(tag);
      if (!closing) {
        for (const attribute of attributes) {
          if (attribute.name === 'xmlns') scope.set('', attribute.value);
          if (attribute.prefix === 'xmlns') scope.set(attribute.localName, attribute.value);
        }
      }
      const tagQName = parseQName(name);
      for (const attribute of attributes) {
        attribute.namespaceUri = attribute.prefix === 'xmlns'
          ? 'http://www.w3.org/2000/xmlns/'
          : attribute.name === 'xmlns'
            ? 'http://www.w3.org/2000/xmlns/'
            // XML default namespaces apply to elements only. An unprefixed
            // attribute is always in no namespace (except xmlns above).
            : attribute.prefix === null
              ? null
              : scope.get(attribute.prefix) ?? null;
      }
      const entry = {
        name,
        localName: tagQName.localName,
        namespaceUri: scope.get(tagQName.prefix ?? '') ?? null,
        start,
        end,
        text: tag,
        attributes,
        scope,
        closing,
        selfClosing: /\/\s*>$/.test(tag),
      };
      tags.push(entry);
      if (!closing && !entry.selfClosing) {
        namespaceStack.push({ name, scope });
      } else if (closing) {
        // XML is expected to be well formed. Pop the matching lexical scope so
        // a local xmlns redeclaration cannot leak into later siblings.
        for (let index = namespaceStack.length - 1; index >= 0; index -= 1) {
          const open = namespaceStack.pop();
          if (open.name === name) break;
        }
      }
    }
    cursor = end;
  }
  return tags;
}

function parseQName(name) {
  const separator = name.indexOf(':');
  return separator === -1
    ? { prefix: null, localName: name }
    : { prefix: name.slice(0, separator), localName: name.slice(separator + 1) };
}

function parseAttributes(tagText) {
  const attributes = [];
  const nameMatch = tagText.match(/^<\/?\s*[A-Za-z_][\w:.-]*/);
  if (!nameMatch) return attributes;

  let index = nameMatch[0].length;
  while (index < tagText.length) {
    while (/\s/.test(tagText[index] ?? '')) index += 1;
    if (tagText[index] === '/' || tagText[index] === '>' || index >= tagText.length) break;

    const attributeMatch = tagText.slice(index).match(/^([A-Za-z_][\w:.-]*)\s*=\s*/);
    if (!attributeMatch) {
      index += 1;
      continue;
    }
    const name = attributeMatch[1];
    index += attributeMatch[0].length;
    const quote = tagText[index];
    if (quote !== '"' && quote !== "'") {
      continue;
    }
    const valueStart = index + 1;
    const valueEnd = tagText.indexOf(quote, valueStart);
    if (valueEnd === -1) break;
    const separator = name.indexOf(':');
    attributes.push({
      name,
      prefix: separator === -1 ? null : name.slice(0, separator),
      localName: separator === -1 ? name : name.slice(separator + 1),
      value: tagText.slice(valueStart, valueEnd),
      start: index - attributeMatch[0].length,
      end: valueEnd + 1,
    });
    index = valueEnd + 1;
  }
  return attributes;
}

function findManifestTag(manifestXml) {
  const manifestTag = scanXmlTags(manifestXml).find(
    (tag) => !tag.closing && tag.name === 'manifest' && tag.namespaceUri === null,
  );
  if (!manifestTag) {
    throw new Error('Target manifest is missing <manifest> root element');
  }
  return manifestTag;
}

function findAndroidNamespacePrefix(manifestXml) {
  const manifestTag = findManifestTag(manifestXml);
  return manifestTag.attributes.find(
    (attribute) => attribute.prefix === 'xmlns' && attribute.value === ANDROID_NAMESPACE_URI,
  )?.localName ?? null;
}

function ensureAndroidNamespace(manifestXml) {
  const existingPrefix = findAndroidNamespacePrefix(manifestXml);
  if (existingPrefix) return { content: manifestXml, prefix: existingPrefix };

  const manifestTag = findManifestTag(manifestXml);
  const insertAt = manifestTag.text.replace(/\s*\/?\s*>$/, '').length;
  const rewrittenTag = `${manifestTag.text.slice(0, insertAt)} xmlns:android="${ANDROID_NAMESPACE_URI}"${manifestTag.text.slice(insertAt)}`;
  return {
    content: `${manifestXml.slice(0, manifestTag.start)}${rewrittenTag}${manifestXml.slice(manifestTag.end)}`,
    prefix: 'android',
  };
}

function hasNamedTag(manifestXml, tagName, androidName) {
  return scanXmlTags(manifestXml).some(
    (tag) => !tag.closing
      && tag.localName === tagName
      && tag.namespaceUri === null
      && tag.attributes.some(
        (attribute) => attribute.namespaceUri === ANDROID_NAMESPACE_URI
          && attribute.localName === 'name'
          && attribute.value === androidName,
      ),
  );
}

function findProviderTagBounds(manifestXml) {
  const tags = scanXmlTags(manifestXml);
  const providerIndex = tags.findIndex(
    (tag) => !tag.closing
      && tag.localName === 'provider'
      && tag.namespaceUri === null
      && tag.attributes.some(
        (attribute) => attribute.namespaceUri === ANDROID_NAMESPACE_URI
          && attribute.localName === 'name'
          && attribute.value === SHIZUKU_PROVIDER_CLASS,
      ),
  );
  if (providerIndex === -1) return null;

  const provider = tags[providerIndex];
  if (provider.selfClosing) return { start: provider.start, end: provider.end, provider };

  let depth = 1;
  for (let index = providerIndex + 1; index < tags.length; index += 1) {
    const tag = tags[index];
    if (tag.name !== 'provider') continue;
    if (!tag.closing && !tag.selfClosing) depth += 1;
    if (tag.closing) depth -= 1;
    if (depth === 0) return { start: provider.start, end: tag.end, provider };
  }
  return null;
}

function selectAndroidPrefix(tag) {
  const nameAttribute = tag.attributes.find(
    (attribute) => attribute.namespaceUri === ANDROID_NAMESPACE_URI
      && attribute.localName === 'name'
      && attribute.prefix,
  );
  if (nameAttribute) return nameAttribute.prefix;
  return [...tag.scope.entries()].find(([, uri]) => uri === ANDROID_NAMESPACE_URI)?.[0] || null;
}

function findOpeningTagEnd(tagText, startIndex = 0) {
  let inQuote = false;
  let quoteChar = '';
  for (let i = startIndex; i < tagText.length; i += 1) {
    const ch = tagText[i];
    if (ch === '"' || ch === "'") {
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

function fixProviderOpeningTag(openingTag, provider, fallbackAndroidPrefix) {
  if (!openingTag.startsWith('<provider')) {
    return openingTag;
  }
  const androidPrefix = selectAndroidPrefix(provider) ?? fallbackAndroidPrefix;
  const expected = new Map([
    ['enabled', 'true'],
    ['exported', 'true'],
    ['multiprocess', 'false'],
    ['permission', 'android.permission.INTERACT_ACROSS_USERS_FULL'],
    ['authorities', '${applicationId}.shizuku'],
  ]);
  const replacements = [];
  const present = new Set();
  for (const attribute of provider.attributes) {
    if (attribute.namespaceUri !== ANDROID_NAMESPACE_URI || !expected.has(attribute.localName)) continue;
    if (present.has(attribute.localName)) {
      throw new Error(`duplicate Android ${attribute.localName} in Shizuku provider tag`);
    }
    present.add(attribute.localName);
    replacements.push({
      start: attribute.start,
      end: attribute.end,
      text: `${attribute.prefix}:${attribute.localName}="${expected.get(attribute.localName)}"`,
    });
  }

  let fixed = openingTag;
  for (const replacement of replacements.sort((a, b) => b.start - a.start)) {
    fixed = `${fixed.slice(0, replacement.start)}${replacement.text}${fixed.slice(replacement.end)}`;
  }
  const missing = [...expected.entries()]
    .filter(([name]) => !present.has(name))
    .map(([name, value]) => `\n            ${androidPrefix}:${name}="${value}"`)
    .join('');
  if (!missing) return fixed;
  const insertAt = fixed.replace(/\s*\/?\s*>$/, '').length;
  return `${fixed.slice(0, insertAt)}${missing}${fixed.slice(insertAt)}`;
}

function fixShizukuProviderBlock(providerBlock, provider, androidPrefix) {
  const providerStart = providerBlock.indexOf('<provider');
  const openEnd = findOpeningTagEnd(providerBlock, providerStart);
  if (openEnd === -1) {
    return providerBlock;
  }

  const openingTag = providerBlock.slice(0, openEnd);
  const remainder = providerBlock.slice(openEnd);
  const fixedOpening = fixProviderOpeningTag(openingTag, provider, androidPrefix);
  return `${fixedOpening}${remainder}`;
}

function fixProviderMultiprocess(manifestXml, androidPrefix) {
  const bounds = findProviderTagBounds(manifestXml);
  if (!bounds) {
    return manifestXml;
  }

  const originalTag = manifestXml.slice(bounds.start, bounds.end);
  const fixedTag = fixShizukuProviderBlock(originalTag, bounds.provider, androidPrefix);
  if (fixedTag === originalTag) {
    return manifestXml;
  }
  return `${manifestXml.slice(0, bounds.start)}${fixedTag}${manifestXml.slice(bounds.end)}`;
}

function replaceAndroidAttributePrefix(xmlSnippet, androidPrefix) {
  return xmlSnippet.replace(/(^|\s)android:/g, `$1${androidPrefix}:`);
}

function findApplicationTag(manifestXml) {
  return scanXmlTags(manifestXml).find(
    (tag) => !tag.closing && tag.name === 'application' && tag.namespaceUri === null,
  );
}

function findApplicationClosingTag(manifestXml) {
  return scanXmlTags(manifestXml).find(
    (tag) => tag.closing && tag.name === 'application' && tag.namespaceUri === null,
  );
}

function selectSnippetAndroidNamespace(applicationTag) {
  const existingPrefix = [...applicationTag.scope.entries()].find(
    ([prefix, uri]) => prefix && uri === ANDROID_NAMESPACE_URI,
  )?.[0];
  if (existingPrefix) return { prefix: existingPrefix, declaration: '' };

  let suffix = '';
  while (applicationTag.scope.has(`openlessAndroid${suffix}`)) {
    suffix = suffix === '' ? '1' : String(Number(suffix) + 1);
  }
  const prefix = `openlessAndroid${suffix}`;
  return { prefix, declaration: ` xmlns:${prefix}="${ANDROID_NAMESPACE_URI}"` };
}

function applySnippetAndroidNamespace(snippet, namespace) {
  const rewritten = replaceAndroidAttributePrefix(snippet, namespace.prefix);
  if (!namespace.declaration) return rewritten;
  return rewritten.replace(/^(<[A-Za-z_][\w:.-]*)/, `$1${namespace.declaration}`);
}

function mergeApplicationChildren(manifestXml) {
  let content = manifestXml;
  let changed = false;
  const applicationTag = findApplicationTag(content);
  if (!applicationTag || !findApplicationClosingTag(content)) {
    throw new Error('Target manifest is missing </application>');
  }
  const namespace = selectSnippetAndroidNamespace(applicationTag);

  for (const entry of APPLICATION_SNIPPETS) {
    if (hasNamedTag(content, entry.tagName, entry.name)) {
      continue;
    }
    const closingIdx = findApplicationClosingTag(content)?.start;
    if (closingIdx === undefined) {
      throw new Error('Target manifest is missing </application>');
    }
    const snippet = applySnippetAndroidNamespace(entry.snippet, namespace);
    content = `${content.slice(0, closingIdx)}        ${snippet}\n${content.slice(closingIdx)}`;
    changed = true;
  }

  return { content, changed };
}

function mergeQueries(manifestXml, androidPrefix) {
  if (hasNamedTag(manifestXml, 'package', SHIZUKU_PACKAGE)) {
    return { content: manifestXml, changed: false };
  }
  const manifestOpen = findManifestTag(manifestXml);
  const insertAt = manifestOpen.end;
  const content =
    `${manifestXml.slice(0, insertAt)}\n    ${replaceAndroidAttributePrefix(QUERIES_SNIPPET, androidPrefix)}\n${manifestXml.slice(insertAt)}`;
  return { content, changed: true };
}

export function mergeShizukuManifest(manifestXml) {
  const before = manifestXml;
  const namespace = ensureAndroidNamespace(manifestXml);
  let content = fixProviderMultiprocess(namespace.content, namespace.prefix);
  content = mergeApplicationChildren(content).content;
  content = mergeQueries(content, namespace.prefix).content;
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
