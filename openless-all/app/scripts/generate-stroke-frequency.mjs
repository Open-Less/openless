import fs from 'node:fs';

const source = process.argv[2];
const output = process.argv[3];
if (!source || !output) throw new Error('usage: node generate-stroke-frequency.mjs SOURCE OUTPUT');

const rows = fs.readFileSync(source, 'utf8').split(/\r?\n/);
const best = new Map();
for (const row of rows) {
  const [character, rawWeight] = row.split('\t');
  const weight = Number(rawWeight);
  if (!character || !Number.isFinite(weight) || [...character].length !== 1) continue;
  if (!/^[㐀-鿿]$/.test(character)) continue;
  const previous = best.get(character);
  if (!previous || weight > previous) best.set(character, weight);
}

const result = [...best.entries()]
  .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
  .map(([character, weight]) => `${character}\t${weight}`)
  .join('\n') + '\n';
fs.writeFileSync(output, result, 'utf8');
console.log(`generated ${best.size} character frequencies`);
