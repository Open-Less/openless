// Generates the Android Pinyin-mode high-frequency abbreviation-phrase
// dictionary asset (android/assets/pinyin_phrases.tsv) from three local,
// commit-pinned source files:
//
//   1. rime/rime-pinyin-simp's pinyin_simp.dict.yaml (Apache-2.0) — the base
//      candidate pool AND its usage-frequency weight (this file mixes
//      single-character and multi-character rows; only multi-character rows
//      are used here — see generate-pinyin-characters.mjs for the single-
//      character half).
//   2. mozillazg/pinyin-data's kMandarin_8105.txt (MIT) — reused only as a
//      membership set (its own character SET, not its readings) to filter
//      out any candidate phrase containing a character outside 《通用规范
//      汉字表》 — i.e. traditional-only, rare, or otherwise non-simplified
//      characters (plan 7.2: "只保留全中文词语" / 6.2: "过滤纯简体中文词语").
//   3. mozillazg/phrase-pinyin-data's pinyin.txt (MIT) — authoritative
//      word-level pinyin readings, used to correct a phrase's reading when
//      it disagrees with rime's own per-character reading (this is what
//      actually resolves "多音字在词语中的正确读音" per plan 6.3 — rime's
//      dict.yaml has no notion of a word, so it can't do this on its own).
//
// Re-fetch the pinned sources with:
//   curl -L -o /tmp/pinyin_simp.dict.yaml \
//     https://raw.githubusercontent.com/rime/rime-pinyin-simp/0c6861ef7420ee780270ca6d993d18d4101049d0/pinyin_simp.dict.yaml
//   curl -L -o /tmp/kMandarin_8105.txt \
//     https://raw.githubusercontent.com/mozillazg/pinyin-data/923b108dc5d45dee061324c011b478fb649f8b73/kMandarin_8105.txt
//   curl -L -o /tmp/phrase_pinyin.txt \
//     https://raw.githubusercontent.com/mozillazg/phrase-pinyin-data/cee0ed6e6e4898580cafd2bd5e3723e20b214aa0/pinyin.txt
//
// Then regenerate the asset with:
//   node scripts/generate-pinyin-phrases.mjs /tmp/pinyin_simp.dict.yaml /tmp/kMandarin_8105.txt /tmp/phrase_pinyin.txt android/assets/pinyin_phrases.tsv [LIMIT=2000]
//
// This is a build/dev-time tool only — the generated TSV is committed and
// read as a plain local asset at runtime; nothing on-device ever fetches
// or reprocesses any of the three sources. A small manually curated
// whitelist (WHITELIST below) adds domain terms ("输入法"/"剪贴板"/etc) the
// generic frequency corpus has no reason to rank highly — plan 7.2 calls
// for exactly this, with the exact word list confirmed by the user before
// being baked into the generator (as opposed to silently guessed).
import fs from 'node:fs';

const rimeSimpPath = process.argv[2];
const kMandarinPath = process.argv[3];
const phrasePinyinPath = process.argv[4];
const output = process.argv[5];
const limit = Number(process.argv[6] ?? 2000);
if (!rimeSimpPath || !kMandarinPath || !phrasePinyinPath || !output) {
  throw new Error('usage: node generate-pinyin-phrases.mjs RIME_SIMP_SOURCE KMANDARIN_SOURCE PHRASE_PINYIN_SOURCE OUTPUT [LIMIT]');
}
const MIN_LENGTH = 2;
const MAX_LENGTH = 4;

// Manually curated domain-term whitelist (plan 7.2: "根据 OpenLess 使用场景
//增加少量人工白名单" — confirmed with the user on 2026-09-25, using the
// plan's own example list verbatim). These are ADDED on top of the
// corpus-ranked top LIMIT, not competed into it — a general-purpose
// frequency corpus has no reason to rank IME/ERP jargon highly, but they're
// exactly the kind of word a stroke-input user reaching for pinyin as a
// fallback is likely typing. Six of the twelve already clear the natural
// weight cutoff on their own (服务器/数据库/客户/订单/生产/审核) and are
// left for the corpus ranking to place normally; only the other six are
// actually injected here. Pinyin is taken from the rime source's own
// per-character weighting where the word exists there at all; 候选词 isn't
// in any of the three sources (it's IME-specific jargon), so its reading is
// hand-supplied — unambiguous, single-reading, no polyphone risk.
const WHITELIST = [
  { word: '输入法', pinyin: 'shu ru fa' },
  { word: '候选词', pinyin: 'hou xuan ci' },
  { word: '剪贴板', pinyin: 'jian tie ban' },
  { word: '供应商', pinyin: 'gong ying shang' },
  { word: '物料', pinyin: 'wu liao' },
  { word: '主管', pinyin: 'zhu guan' },
];
// Synthetic weight for a whitelist word this generator can't find any real
// corpus weight for (currently just 候选词) — deliberately low so it never
// outranks a genuinely common word, just guarantees presence in the list.
const WHITELIST_FALLBACK_WEIGHT = 500;

// Manually curated corpus-noise blacklist — words that are valid simplified
// Chinese and technically common in the source corpus but aren't real
// day-to-day vocabulary (rime-pinyin-simp is derived from an old Android
// IME's usage corpus, which includes forum/UI boilerplate text). Found by
// spot-checking the generated top-2000 list, not an exhaustive audit — see
// docs/pinyin-lite/phase-0-audit.md's sibling phase notes. Removed from the
// candidate pool entirely (so it can't be re-surfaced by a future weight
// change) rather than just skipped at output time.
const BLACKLIST = new Set([
  '回复日期', // forum/IME-corpus UI template text, not a phrase anyone actually types — ranked #11 in the very first generation run despite that
]);

function toAsciiPinyin(raw) {
  const uUmlaut = raw.replace(/[üǖǘǚǜÜǕǗǙǛ]/g, (ch) => (ch === ch.toUpperCase() ? 'V' : 'v'));
  return uUmlaut
    .normalize('NFD')
    .replace(/[̀-ͯ]/g, '')
    .toLowerCase();
}

function abbreviate(syllables) {
  return syllables.map((s) => s[0] ?? '').join('');
}

// --- 1. kMandarin_8105.txt character-set membership (readings not needed here) ---
const CODEPOINT_LINE = /^U\+([0-9A-Fa-f]+):/;
const simplifiedCharSet = new Set();
for (const line of fs.readFileSync(kMandarinPath, 'utf8').split(/\r?\n/)) {
  const match = CODEPOINT_LINE.exec(line);
  if (match) simplifiedCharSet.add(String.fromCodePoint(parseInt(match[1], 16)));
}
const isAllSimplified = (word) => [...word].every((ch) => simplifiedCharSet.has(ch));

// --- 2. phrase-pinyin-data pinyin.txt: "词语: py1 py2 py3" authoritative readings ---
const authoritativeReading = new Map(); // word -> asciiPinyin[]
for (const line of fs.readFileSync(phrasePinyinPath, 'utf8').split(/\r?\n/)) {
  const colon = line.indexOf(':');
  if (colon === -1 || line.startsWith('#')) continue;
  const word = line.slice(0, colon).trim();
  const syllables = line.slice(colon + 1).trim().split(/\s+/).map(toAsciiPinyin).filter(Boolean);
  if (word && syllables.length === [...word].length) authoritativeReading.set(word, syllables);
}

// --- 3. rime_simp multi-character rows: base candidate pool + frequency weight ---
const candidates = new Map(); // word -> { syllables: string[], weight: number }
let inDataSection = false;
for (const line of fs.readFileSync(rimeSimpPath, 'utf8').split(/\r?\n/)) {
  if (!inDataSection) {
    if (line.trim() === '...') inDataSection = true;
    continue;
  }
  const parts = line.split('\t');
  if (parts.length < 3) continue;
  const word = parts[0];
  const length = [...word].length;
  if (length < MIN_LENGTH || length > MAX_LENGTH) continue;
  if (!isAllSimplified(word)) continue;
  if (BLACKLIST.has(word)) continue;
  const weight = Number(parts[2]);
  if (!Number.isFinite(weight) || weight <= 0) continue;
  const rimeSyllables = parts[1].trim().split(/\s+/).map(toAsciiPinyin).filter(Boolean);
  if (rimeSyllables.length !== length) continue;
  // Prefer the authoritative phrase-level reading when available (resolves
  // polyphones-in-context rime's own per-character weighting can't see);
  // fall back to rime's own reading otherwise.
  const syllables = authoritativeReading.get(word) ?? rimeSyllables;
  const previous = candidates.get(word);
  if (!previous || weight > previous.weight) candidates.set(word, { syllables, weight });
}

// --- 4. Rank by weight, keep the top LIMIT ---
const rankedEntries = [...candidates.entries()]
  .sort((a, b) => b[1].weight - a[1].weight || a[0].localeCompare(b[0]))
  .slice(0, limit);
const ranked = new Map(rankedEntries);

// --- 5. Add any whitelist word not already present, on top of the LIMIT cap ---
for (const { word, pinyin } of WHITELIST) {
  if (ranked.has(word)) continue;
  const syllables = pinyin.split(/\s+/).map(toAsciiPinyin);
  const weight = candidates.get(word)?.weight ?? WHITELIST_FALLBACK_WEIGHT;
  ranked.set(word, { syllables, weight });
}

const rows = [...ranked.entries()].map(([word, { syllables, weight }]) => {
  const fullPinyin = syllables.join(' ');
  const abbreviation = abbreviate(syllables);
  return `${word}\t${fullPinyin}\t${abbreviation}\t${weight}`;
});
fs.writeFileSync(output, rows.join('\n') + '\n', 'utf8');

const byLength = new Map();
for (const [word] of ranked) byLength.set([...word].length, (byLength.get([...word].length) ?? 0) + 1);
console.log(`generated ${ranked.size} phrases (of ${candidates.size} candidates in source), by length: ${[...byLength.entries()].sort().map(([l, n]) => `${l}字=${n}`).join(', ')}`);
