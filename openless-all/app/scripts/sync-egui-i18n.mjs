#!/usr/bin/env node
// Sync the Linux egui localization catalog with the Tauri i18n catalogs.
//
// The two UIs ship as one product and support the same five locales, but they
// keep separate catalogs: the Tauri app uses `src/i18n/*.ts`, the egui host uses
// the Rust table in `linux-egui/src/i18n.rs`. This script removes the duplicated
// translation work: for every key the egui catalog shares with the Tauri one it
// copies the five translations over, so translators only maintain the Tauri
// files.
//
// Matching ignores case, `.` and `_`, so `overview.week_days` picks up the
// Tauri `overview.weekDays` value and `nav.polish_mode` picks up
// `nav.polishMode`. Tauri's `{{name}}` placeholders become the positional `{}`
// the Rust `fmt_catalog` expects, and array values (weekday/month label lists)
// are joined with `|`, which is the encoding the egui pages split on.
//
// Run with: node scripts/sync-egui-i18n.mjs [--check]
//   --check  report drift and exit non-zero instead of rewriting the catalog

import { readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const APP_ROOT = resolve(HERE, '..');
const CATALOG_PATH = resolve(APP_ROOT, 'linux-egui/src/i18n.rs');

// Rust row order: [zh-CN, zh-TW, en, ja, ko].
const LOCALES = [
  ['zh-CN', 'zhCN'],
  ['zh-TW', 'zhTW'],
  ['en', 'en'],
  ['ja', 'ja'],
  ['ko', 'ko'],
];

const RUST_LOCALE_ORDER = ['zh-CN', 'zh-TW', 'en', 'ja', 'ko'];

/// Keys where the egui host deliberately keeps its own wording and must not be
/// overwritten by the Tauri value (different product surface / screenshot parity).
const KEEP_EGUI = new Set([
  // Screenshot labels the sidebar entry "纠错规则"; Tauri uses "纠正规则".
  'nav.corrections',
  // Screenshot labels the detail action "重新转写"; Tauri uses "重新转录".
  'history.retranscribe',
  // The egui status line prefixes the product name.
  'less_computer.done',
]);

function placeholderCount(text) {
  return (text.match(/\{\}/g) ?? []).length;
}

function normalizeKey(key) {
  return key.toLowerCase().replace(/[._]/g, '');
}

function flatten(value, prefix, out) {
  for (const [key, entry] of Object.entries(value)) {
    const path = prefix ? `${prefix}.${key}` : key;
    if (Array.isArray(entry)) {
      out.set(path, entry.join('|'));
    } else if (entry && typeof entry === 'object') {
      flatten(entry, path, out);
    } else if (typeof entry === 'string') {
      out.set(path, entry);
    }
  }
  return out;
}

async function loadTauriCatalog() {
  const byLocale = new Map();
  for (const [tag, exportName] of LOCALES) {
    const module = await import(resolve(APP_ROOT, `src/i18n/${tag}.ts`));
    byLocale.set(tag, flatten(module[exportName], '', new Map()));
  }
  // normalized key -> { tag -> text }
  const shared = new Map();
  const primary = byLocale.get('zh-CN');
  for (const [key, text] of primary) {
    const entry = { 'zh-CN': text };
    let complete = true;
    for (const tag of RUST_LOCALE_ORDER) {
      if (tag === 'zh-CN') continue;
      const value = byLocale.get(tag).get(key);
      if (value === undefined) {
        complete = false;
        break;
      }
      entry[tag] = value;
    }
    if (complete) shared.set(normalizeKey(key), entry);
  }
  return shared;
}

/** Convert a Tauri string into the Rust catalog form. */
function toRustText(text) {
  return text
    // React markup used only for emphasis in the Tauri UI has no egui meaning.
    .replace(/<\/?[a-zA-Z][^>]*>/g, '')
    .replace(/\{\{\s*[^}]+?\s*\}\}/g, '{}');
}

function rustEscape(text) {
  return text.replace(/\\/g, '\\\\').replace(/"/g, '\\"').replace(/\n/g, '\\n').replace(/\r/g, '\\r');
}

function quote(text) {
  return `"${rustEscape(text)}"`;
}

// ── Rust catalog scanner ────────────────────────────────────────────────────

/** Index of the matching close for the opening bracket at `open`. */
function matchBracket(source, open, openChar, closeChar) {
  let depth = 0;
  let i = open;
  let inString = false;
  while (i < source.length) {
    const ch = source[i];
    if (inString) {
      if (ch === '\\') i += 1;
      else if (ch === '"') inString = false;
    } else if (ch === '"') {
      inString = true;
    } else if (ch === openChar) {
      depth += 1;
    } else if (ch === closeChar) {
      depth -= 1;
      if (depth === 0) return i;
    }
    i += 1;
  }
  throw new Error(`unbalanced ${openChar} at ${open}`);
}

/** Read the Rust string literal starting at `start` (which must be `"`). */
function readString(source, start) {
  let out = '';
  let i = start + 1;
  while (i < source.length) {
    const ch = source[i];
    if (ch === '\\') {
      const next = source[i + 1];
      if (next === 'n') out += '\n';
      else if (next === 'r') out += '\r';
      else if (next === 't') out += '\t';
      else out += next;
      i += 2;
      continue;
    }
    if (ch === '"') return { text: out, end: i };
    out += ch;
    i += 1;
  }
  throw new Error('unterminated string literal');
}

/** All `Msg { key: "…", text: row(…) }` entries with their source ranges. */
function scanMessages(source) {
  const entries = [];
  const keyPattern = /key:\s*"/g;
  let match;
  while ((match = keyPattern.exec(source)) !== null) {
    // `match[0]` includes the opening quote; point `readString` at it.
    const keyStart = match.index + match[0].length - 1;
    const key = readString(source, keyStart);
    const blockStart = source.lastIndexOf('Msg {', keyStart);
    const braceStart = source.indexOf('{', blockStart);
    const blockEnd = matchBracket(source, braceStart, '{', '}');
    const rowStart = source.indexOf('row(', keyStart);
    if (rowStart === -1 || rowStart > blockEnd) continue;
    const parenStart = rowStart + 'row'.length;
    const parenEnd = matchBracket(source, parenStart, '(', ')');

    const values = [];
    let i = parenStart + 1;
    while (i < parenEnd) {
      const ch = source[i];
      if (ch === '"') {
        const literal = readString(source, i);
        values.push(literal.text);
        i = literal.end + 1;
        continue;
      }
      i += 1;
    }
    // Consume the block's trailing comma too; `renderBlock` re-emits it.
    const end = source[blockEnd + 1] === ',' ? blockEnd + 2 : blockEnd + 1;
    entries.push({ key: key.text, values, start: blockStart, end });
  }
  return entries;
}

function renderBlock(key, values) {
  const oneLine = `    Msg {\n        key: "${key}",\n        text: row(${values.map(quote).join(', ')}),\n    },`;
  const anyLong = values.some((value) => value.length > 12);
  if (!anyLong) return oneLine;
  return [
    '    Msg {',
    `        key: "${key}",`,
    '        text: row(',
    ...values.map((value) => `            ${quote(value)},`),
    '        ),',
    '    },',
  ].join('\n');
}

/** `tr_l10n(…, "key")` / `fmt_l10n(…, "key", …)` call sites under `src/`. */
function referencedKeys() {
  const keys = new Set();
  const files = [];
  const walk = (directory) => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const path = resolve(directory, entry.name);
      if (entry.isDirectory()) walk(path);
      else if (entry.name.endsWith('.rs')) files.push(path);
    }
  };
  walk(resolve(APP_ROOT, 'linux-egui/src'));
  const pattern = /\b(?:tr_l10n|fmt_l10n)\(\s*[^,()]+,\s*"([^"]+)"/g;
  for (const file of files) {
    const source = readFileSync(file, 'utf8');
    let match;
    while ((match = pattern.exec(source)) !== null) keys.add(match[1]);
  }
  return keys;
}

/** Insert index for new entries: just before the CATALOG slice terminator. */
function catalogInsertIndex(source) {
  const start = source.indexOf('pub const CATALOG');
  if (start === -1) throw new Error('CATALOG array not found in i18n.rs');
  const end = source.indexOf('\n];', start);
  if (end === -1) throw new Error('CATALOG array terminator not found');
  return end + 1;
}

async function main() {
  const check = process.argv.includes('--check');
  const shared = await loadTauriCatalog();
  const source = readFileSync(CATALOG_PATH, 'utf8');
  const messages = scanMessages(source);

  const rewritten = [];
  let updated = 0;
  let unchanged = 0;
  let skipped = 0;
  let guarded = 0;
  let cursor = 0;
  for (const message of messages) {
    rewritten.push(source.slice(cursor, message.start));
    const tauri = shared.get(normalizeKey(message.key));
    if (!tauri || KEEP_EGUI.has(message.key)) {
      skipped += 1;
      rewritten.push(source.slice(message.start, message.end));
      cursor = message.end;
      continue;
    }
    const values = RUST_LOCALE_ORDER.map((tag) => toRustText(tauri[tag]));
    // Never take a translation whose placeholders no longer line up with the
    // arguments the egui pages pass; that would render a literal `{}`.
    const expected = message.values.map(placeholderCount);
    const mismatch = values.some(
      (value, index) => placeholderCount(value) !== expected[index],
    );
    if (mismatch) {
      guarded += 1;
      if (check) console.log(`skipped (placeholder mismatch): ${message.key}`);
      rewritten.push(source.slice(message.start, message.end));
      cursor = message.end;
      continue;
    }
    const same =
      values.length === message.values.length &&
      values.every((value, index) => value === message.values[index]);
    if (same) {
      unchanged += 1;
      rewritten.push(source.slice(message.start, message.end));
    } else {
      updated += 1;
      if (check) {
        console.log(`drift: ${message.key}`);
        console.log(`  egui:  ${JSON.stringify(message.values)}`);
        console.log(`  tauri: ${JSON.stringify(values)}`);
      }
      rewritten.push(renderBlock(message.key, values));
    }
    cursor = message.end;
  }
  rewritten.push(source.slice(cursor));
  let output = rewritten.join('');

  // Add catalog rows for keys the egui sources reference but the catalog lacks.
  const known = new Set(messages.map((message) => message.key));
  const missing = [];
  for (const key of referencedKeys()) {
    if (known.has(key)) continue;
    const tauri = shared.get(normalizeKey(key));
    if (!tauri) missing.push(key);
  }
  const additions = [];
  for (const key of [...referencedKeys()].sort()) {
    if (known.has(key)) continue;
    const tauri = shared.get(normalizeKey(key));
    if (!tauri) continue;
    additions.push(renderBlock(key, RUST_LOCALE_ORDER.map((tag) => toRustText(tauri[tag]))));
  }
  if (additions.length > 0) {
    const at = catalogInsertIndex(output);
    output = `${output.slice(0, at)}${additions.join('\n')}\n${output.slice(at)}`;
  }

  console.log(
    `egui i18n catalog: ${messages.length} keys · ${updated} updated · ${unchanged} in sync · ` +
      `${skipped} egui-only/kept · ${guarded} skipped (placeholder mismatch) · ` +
      `${additions.length} added`,
  );
  if (missing.length > 0) {
    console.error(
      `referenced keys with no Tauri translation (add them by hand):\n  ${missing.join('\n  ')}`,
    );
  }

  if (check) {
    if (updated > 0 || additions.length > 0) {
      console.error('Catalog is out of sync with the Tauri i18n files.');
      process.exit(1);
    }
    return;
  }
  if (updated > 0 || additions.length > 0) {
    writeFileSync(CATALOG_PATH, output);
    console.log(`wrote ${CATALOG_PATH}`);
  }
}

await main();
