// Generates the Android Pinyin-mode single-character dictionary asset
// (android/assets/pinyin_chars.tsv) from three local, commit-pinned source
// files:
//
//   1. mozillazg/pinyin-data's kMandarin_8105.txt (MIT) — the canonical
//      character SET and each character's single primary reading, scoped
//      to 《通用规范汉字表》(2013), per the lite-pinyin plan's own 7.1
//      requirement. This file deliberately records only one reading per
//      character (its own stated purpose), so it cannot supply polyphones
//      by itself — see (2) below.
//   2. mozillazg/pinyin-data's pinyin.txt (MIT) — the same project's full
//      Unihan-derived reading list, comma-separated per character, used
//      only to supplement ADDITIONAL readings for characters already
//      selected via kMandarin_8105.txt (plan 6.1: "从 pinyin.txt 补充多音字").
//   3. rime/rime-pinyin-simp's pinyin_simp.dict.yaml (Apache-2.0) — supplies
//      real usage-frequency weights per (character, reading) pair, which
//      neither pinyin-data file has. Also decides which of a polyphone's
//      extra readings from (2) are actually "常用" (commonly used) enough
//      to keep (plan 7.1 point 4) — any extra reading below
//      MIN_ALT_READING_WEIGHT is dropped as too rare to be worth a
//      candidate-list slot.
//
// Re-fetch the pinned sources with:
//   curl -L -o /tmp/kMandarin_8105.txt \
//     https://raw.githubusercontent.com/mozillazg/pinyin-data/923b108dc5d45dee061324c011b478fb649f8b73/kMandarin_8105.txt
//   curl -L -o /tmp/pinyin_full.txt \
//     https://raw.githubusercontent.com/mozillazg/pinyin-data/923b108dc5d45dee061324c011b478fb649f8b73/pinyin.txt
//   curl -L -o /tmp/pinyin_simp.dict.yaml \
//     https://raw.githubusercontent.com/rime/rime-pinyin-simp/0c6861ef7420ee780270ca6d993d18d4101049d0/pinyin_simp.dict.yaml
//
// Then regenerate the asset with:
//   node scripts/generate-pinyin-characters.mjs /tmp/kMandarin_8105.txt /tmp/pinyin_full.txt /tmp/pinyin_simp.dict.yaml android/assets/pinyin_chars.tsv [LIMIT=5000]
//
// This is a build/dev-time tool only — the generated TSV is committed and
// read as a plain local asset at runtime; nothing on-device ever fetches
// or reprocesses any of the three sources.
import fs from 'node:fs';

const kMandarinPath = process.argv[2];
const pinyinFullPath = process.argv[3];
const rimeSimpPath = process.argv[4];
const output = process.argv[5];
const limit = Number(process.argv[6] ?? 5000);
if (!kMandarinPath || !pinyinFullPath || !rimeSimpPath || !output) {
  throw new Error('usage: node generate-pinyin-characters.mjs KMANDARIN_SOURCE PINYIN_FULL_SOURCE RIME_SIMP_SOURCE OUTPUT [LIMIT]');
}
// An alternate (non-primary) reading needs at least this much rime-pinyin-simp
// usage weight to be considered "常用" and kept as a second/third candidate
// reading; chosen empirically against known polyphones (行 hang=3209, 长
// chang=13911, 重 chong=4543 all comfortably clear this; 落 lao=23 does not).
const MIN_ALT_READING_WEIGHT = 100;
// Hard cap on readings kept per character, primary reading included —
// avoids a long tail of technically-real but rarely-useful readings for
// the few characters with unusually many alternates.
const MAX_READINGS_PER_CHAR = 3;

// Strips tone marks to plain ASCII pinyin. ü (and its four toned forms) is
// mapped to 'v' rather than 'u', matching the plan's "ü、v 等规范化" test
// requirement and the convention every Chinese IME uses on a QWERTY layout
// (there's no ü key).
function toAsciiPinyin(raw) {
  const uUmlaut = raw.replace(/[üǖǘǚǜÜǕǗǙǛ]/g, (ch) => (ch === ch.toUpperCase() ? 'V' : 'v'));
  return uUmlaut
    .normalize('NFD')
    .replace(/[̀-ͯ]/g, '')
    .toLowerCase();
}

// --- 1. Parse kMandarin_8105.txt: "U+XXXX: pinyin  # 字 ..." (one primary reading) ---
// The character is reconstructed from the codepoint (not scraped from the
// trailing comment) since that field sometimes carries extra "=> U+xxxx"
// (variant-character) or "?-> altpinyin" (disputed-reading) annotations
// that aren't part of the character itself.
const CODEPOINT_LINE = /^U\+([0-9A-Fa-f]+):\s*([^#]+?)\s*#/;
const primaryReading = new Map(); // char -> asciiPinyin (this file's one reading per char)
for (const line of fs.readFileSync(kMandarinPath, 'utf8').split(/\r?\n/)) {
  const match = CODEPOINT_LINE.exec(line);
  if (!match) continue;
  const char = String.fromCodePoint(parseInt(match[1], 16));
  // kMandarin_8105.txt is documented as one reading per line, but defensively
  // take only the first if a stray comma ever appears.
  const reading = toAsciiPinyin(match[2].split(',')[0].trim());
  if (reading) primaryReading.set(char, reading);
}

// --- 2. Parse pinyin.txt: "U+XXXX: pinyin1,pinyin2,...  # 字" (all known readings) ---
// Only used to look up extra readings for characters already selected via
// kMandarin_8105.txt above — this file's own character coverage (all of
// Unihan) is far broader than the 《通用规范汉字表》 scope this feature wants.
const allReadings = new Map(); // char -> Set<asciiPinyin>
for (const line of fs.readFileSync(pinyinFullPath, 'utf8').split(/\r?\n/)) {
  const match = CODEPOINT_LINE.exec(line);
  if (!match) continue;
  const char = String.fromCodePoint(parseInt(match[1], 16));
  const readings = match[2].split(',').map((p) => toAsciiPinyin(p.trim())).filter(Boolean);
  if (readings.length) allReadings.set(char, new Set(readings));
}

// --- 3. Parse pinyin_simp.dict.yaml: "字\tpinyin\tweight" data rows only ---
// (single-character rows only here; the same file's multi-character rows
// feed generate-pinyin-phrases.mjs instead.)
const weightByCharPinyin = new Map(); // "char|pinyin" -> max weight seen
let inDataSection = false;
for (const line of fs.readFileSync(rimeSimpPath, 'utf8').split(/\r?\n/)) {
  if (!inDataSection) {
    if (line.trim() === '...') inDataSection = true;
    continue;
  }
  const parts = line.split('\t');
  if (parts.length < 3) continue;
  const word = parts[0];
  if ([...word].length !== 1) continue;
  const pinyin = toAsciiPinyin(parts[1].trim());
  const weight = Number(parts[2]);
  if (!Number.isFinite(weight)) continue;
  const key = `${word}|${pinyin}`;
  const previous = weightByCharPinyin.get(key);
  if (previous === undefined || weight > previous) weightByCharPinyin.set(key, weight);
}

// --- 4. For each kMandarin character, build its kept reading list: ---
// the primary reading always, plus any other reading from pinyin.txt whose
// rime weight clears MIN_ALT_READING_WEIGHT, capped at MAX_READINGS_PER_CHAR
// total and ranked by weight (primary reading wins ties so it's never
// bumped out by the cap).
const weightOf = (char, reading) => weightByCharPinyin.get(`${char}|${reading}`) ?? 0;
const keptReadings = new Map(); // char -> [{reading, weight}], sorted by weight desc
const charBestWeight = new Map();
for (const [char, primary] of primaryReading) {
  const candidates = new Map(); // reading -> weight
  candidates.set(primary, weightOf(char, primary));
  for (const reading of allReadings.get(char) ?? []) {
    if (reading === primary) continue;
    const weight = weightOf(char, reading);
    if (weight >= MIN_ALT_READING_WEIGHT) candidates.set(reading, weight);
  }
  const ranked = [...candidates.entries()]
    .sort((a, b) => (b[0] === primary ? 1 : 0) - (a[0] === primary ? 1 : 0) || b[1] - a[1])
    .slice(0, MAX_READINGS_PER_CHAR);
  // Re-sort for output (weight desc; primary's own weight may be 0 and
  // legitimately sort last among kept readings — it's still always present).
  ranked.sort((a, b) => b[1] - a[1]);
  keptReadings.set(char, ranked.map(([reading, weight]) => ({ reading, weight })));
  charBestWeight.set(char, Math.max(...ranked.map(([, weight]) => weight)));
}

// --- 5. Rank characters by their best reading's weight, keep the top LIMIT ---
const rankedChars = [...charBestWeight.entries()]
  .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
  .slice(0, limit)
  .map(([char]) => char);

// --- 6. Emit one row per (char, reading) for every kept character ---
const rows = [];
for (const char of rankedChars) {
  for (const { reading, weight } of keptReadings.get(char)) rows.push([char, reading, weight]);
}
rows.sort((a, b) => b[2] - a[2] || a[0].localeCompare(b[0]) || a[1].localeCompare(b[1]));

const result = rows.map(([char, pinyin, weight]) => `${char}\t${pinyin}\t${weight}`).join('\n') + '\n';
fs.writeFileSync(output, result, 'utf8');
console.log(`generated ${rankedChars.length} characters (${rows.length} character+reading rows, of ${primaryReading.size} candidates in source)`);
