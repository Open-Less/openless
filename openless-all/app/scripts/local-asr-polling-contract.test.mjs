import { readFile } from 'node:fs/promises';

const source = await readFile(
  new URL('../src/pages/LocalAsr/index.tsx', import.meta.url),
  'utf-8',
);

const refreshPolling = source.match(
  /window\.setInterval\(\(\) => \{\s*void refresh\(\)\s*\}, 3000\)/g,
) ?? [];

if (refreshPolling.length !== 1) {
  throw new Error(`LocalAsr should have one refresh poller, found ${refreshPolling.length}`);
}

if (!/if \(downloadDialogOpen\) return[\s\S]{0,200}window\.setInterval\(\(\) => \{\s*void refresh\(\)/.test(source)) {
  throw new Error('LocalAsr refresh polling must stop while the download dialog is open');
}

for (const contract of [
  'const downloadDialogOpenRef = useRef(downloadDialogOpen)',
  'const refreshGenerationRef = useRef(0)',
  'const makeRefreshGuard = (): RefreshGuard =>',
  'refreshGenerationRef.current += 1',
  'if (!isCurrent()) return',
]) {
  if (!source.includes(contract)) {
    throw new Error(`LocalAsr refresh guard contract is missing: ${contract}`);
  }
}

console.log('LocalAsr keeps one refresh poller and pauses it for the download dialog');
