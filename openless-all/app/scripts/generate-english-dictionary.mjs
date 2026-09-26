// Generates the Android English keyboard's base word-frequency dictionary
// asset (android/assets/english-frequency.tsv) from a local copy of
// hermitdave/FrequencyWords' English OpenSubtitles-2018 frequency list
// (MIT licensed — see android/assets/english-frequency.LICENSE.txt).
//
// Re-fetch the source with:
//   curl -L -o /tmp/en_50k.txt \
//     https://raw.githubusercontent.com/hermitdave/FrequencyWords/master/content/2018/en/en_50k.txt
//
// Then regenerate the asset with:
//   node scripts/generate-english-dictionary.mjs /tmp/en_50k.txt android/assets/english-frequency.tsv
//
// This is a build/dev-time tool only — the generated TSV is committed and
// read as a plain local asset at runtime; nothing on-device ever fetches
// or reprocesses the source list.
import fs from 'node:fs';

const source = process.argv[2];
const output = process.argv[3];
const limit = Number(process.argv[4] ?? 20000);
if (!source || !output) {
  throw new Error('usage: node generate-english-dictionary.mjs SOURCE OUTPUT [LIMIT]');
}

// Subtitle-derived tokenization splits contractions into fragments
// ("don't" -> "don" + "'t"). Fragments starting with an apostrophe are
// rejected by WORD_PATTERN below (it requires starting with a letter);
// "n't" is the one letter-first fragment common enough to need an explicit
// exclusion. Single-letter entries are rejected except the two that are
// genuinely common standalone English words.
const WORD_PATTERN = /^[a-z]+(?:['-][a-z]+)*$/;
const EXCLUDE = new Set(["n't"]);
const SINGLE_LETTER_ALLOW = new Set(['a', 'i']);

const lines = fs.readFileSync(source, 'utf8').split(/\r?\n/);
const rows = [];
for (const line of lines) {
  const [rawWord, rawCount] = line.trim().split(/\s+/);
  if (!rawWord || rawCount === undefined) continue;
  const word = rawWord.toLowerCase();
  const count = Number(rawCount);
  if (!Number.isFinite(count) || count <= 0) continue;
  if (word.length < 2 && !SINGLE_LETTER_ALLOW.has(word)) continue;
  if (EXCLUDE.has(word)) continue;
  if (!WORD_PATTERN.test(word)) continue;
  rows.push([word, count]);
}

// Source is already sorted descending by count; de-dupe defensively
// (case-folding two differently-cased source rows into the same word)
// by keeping the higher count.
const best = new Map();
for (const [word, count] of rows) {
  const previous = best.get(word);
  if (!previous || count > previous) best.set(word, count);
}

const result = [...best.entries()]
  .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
  .slice(0, limit)
  .map(([word, count]) => `${word}\t${count}`)
  .join('\n') + '\n';
fs.writeFileSync(output, result, 'utf8');
console.log(`generated ${Math.min(best.size, limit)} English word frequencies (of ${best.size} candidates)`);
