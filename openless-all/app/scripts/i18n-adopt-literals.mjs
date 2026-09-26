#!/usr/bin/env node
// Adopt hardcoded Simplified-Chinese literals into the egui i18n catalog.
//
// The legacy egui pages hardcode zh-CN strings. The Tauri catalogs already hold
// translations for the same product surfaces, so a literal whose text matches a
// Tauri zh-CN value can be replaced by a `tr_l10n(lang, "key")` call without a
// translator touching it.
//
// Usage: node --experimental-strip-types --import ./scripts/register-ts-loader.mjs \
//          scripts/i18n-adopt-literals.mjs <file.rs> [...]
//
// The script only rewrites whole string literals that contain CJK and no `{}`
// placeholders (format templates need `fmt_l10n` and a hand-written call).
// Anything ambiguous (the same zh text under several keys) or unmatched is
// reported instead of guessed at.

import { readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const APP_ROOT = resolve(new URL('..', import.meta.url).pathname);
const files = process.argv.slice(2);
if (files.length === 0) {
  console.error('usage: i18n-adopt-literals.mjs <file.rs> [...]');
  process.exit(2);
}

const zhCN = (await import(resolve(APP_ROOT, 'src/i18n/zh-CN.ts'))).zhCN;

/** zh text -> [dotted keys] */
const valueToKeys = new Map();
function walk(value, prefix) {
  for (const [key, entry] of Object.entries(value)) {
    const path = prefix ? `${prefix}.${key}` : key;
    if (Array.isArray(entry)) continue; // arrays need the joining convention
    if (entry && typeof entry === 'object') walk(entry, path);
    else if (typeof entry === 'string') {
      const list = valueToKeys.get(entry) ?? [];
      list.push(path);
      valueToKeys.set(entry, list);
    }
  }
}
walk(zhCN, '');

const CJK = /[\u4e00-\u9fff]/;

/// These literals are used as `match` patterns for action dispatch, so they
/// cannot be replaced by a function call. They are listed in the localization
/// baseline and will be translated when the dispatch moves to typed ids.
const SKIP_LITERALS = new Set(['导出错误日志', '打开 GitHub']);
const cache = new Map();
function toSnake(dotted) {
  if (cache.has(dotted)) return cache.get(dotted);
  const result = dotted
    .split('.')
    .map((part) => part.replace(/([a-z0-9])([A-Z])/g, '$1_$2').toLowerCase())
    .join('.');
  cache.set(dotted, result);
  return result;
}

let totalReplaced = 0;
const unresolved = new Map(); // literal -> reason
for (const file of files) {
  const path = resolve(process.cwd(), file);
  const source = readFileSync(path, 'utf8');
  let out = '';
  let i = 0;
  let replaced = 0;
  while (i < source.length) {
    const ch = source[i];
    if (ch !== '"') {
      out += ch;
      i += 1;
      continue;
    }
    // Read the full literal, tracking escapes.
    let j = i + 1;
    let raw = '';
    while (j < source.length && source[j] !== '"') {
      if (source[j] === '\\') {
        raw += source[j] + (source[j + 1] ?? '');
        j += 2;
        continue;
      }
      raw += source[j];
      j += 1;
    }
    const literal = raw;
    const text = literal
      .replace(/\\n/g, '\n')
      .replace(/\\"/g, '"')
      .replace(/\\\\/g, '\\');
    if (SKIP_LITERALS.has(text) || !CJK.test(text) || text.includes('{') || text.includes('}')) {
      out += source.slice(i, j + 1);
      i = j + 1;
      continue;
    }
    const keys = valueToKeys.get(text);
    if (!keys || keys.length === 0) {
      if (!unresolved.has(text)) unresolved.set(text, 'no Tauri key with this zh text');
      out += source.slice(i, j + 1);
      i = j + 1;
      continue;
    }
    const unique = [...new Set(keys.map(toSnake))];
    if (unique.length > 1) {
      if (!unresolved.has(text)) {
        unresolved.set(text, `ambiguous: ${unique.join(', ')}`);
      }
      out += source.slice(i, j + 1);
      i = j + 1;
      continue;
    }
    out += `tr_l10n(lang, "${unique[0]}")`;
    replaced += 1;
    i = j + 1;
  }
  if (replaced > 0) writeFileSync(path, out);
  console.log(`${file}: ${replaced} literal(s) adopted`);
  totalReplaced += replaced;
}

console.log(`total adopted: ${totalReplaced}`);
if (unresolved.size > 0) {
  console.log(`\nneeds hand-written keys (${unresolved.size}):`);
  for (const [text, reason] of unresolved) {
    console.log(`  ${JSON.stringify(text)} — ${reason}`);
  }
}
