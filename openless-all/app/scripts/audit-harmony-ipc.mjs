import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const root = fileURLToPath(new URL('../', import.meta.url));
const ipc = 'src/lib/ipc';
const directories = fs
  .readdirSync(path.join(root, ipc))
  .filter((file) => file.endsWith('.ts') && !file.endsWith('.test.ts') && file !== 'mock-data.ts')
  .map((file) => `${ipc}/${file}`);
const staticSources = [
  ...directories,
  'src/api/in-app-dictation.ts',
  'src/runtime/files.ts',
  'src/runtime/platform.ts',
  'src/runtime/transport.ts',
];
const calls = new Map();
for (const file of staticSources) {
  const text = fs.readFileSync(path.join(root, file), 'utf8');
  for (const pattern of [
    /\b(?:invokeOrMock|invokeOrPreview|invoke|invokeHarmony)(?:<[^;]*?>)?\(\s*['"]([a-z][a-z0-9_]+)['"]/g,
    /\binvokeNative(?:<[^;]*?>)?\(\s*host\s*,\s*['"]([a-z][a-z0-9_]+)['"]/g,
  ]) {
    for (const match of text.matchAll(pattern)) {
      const name = match[1];
      if (!calls.has(name)) calls.set(name, []);
      calls.get(name).push(file);
    }
  }
}
const rust = fs.readFileSync(path.join(root, 'harmony/rust/src/lib.rs'), 'utf8');
const supported = new Map();
for (const arm of rust.matchAll(/^\s*((?:"[a-z][a-z0-9_]+"\s*\|\s*)*"[a-z][a-z0-9_]+")\s*=>/gm)) {
  for (const name of arm[1].matchAll(/"([^"]+)"/g)) supported.set(name[1], 'rust');
}
const ark = fs.readFileSync(path.join(root, 'harmony/entry/src/main/ets/HarmonyApi.ets'), 'utf8');
for (const match of ark.matchAll(/case '([a-z][a-z0-9_]+)'\s*:/g)) supported.set(match[1], 'arkts');
for (const name of ['get_windows_ime_status', 'get_hotkey_status', 'get_hotkey_capability'])
  supported.set(name, 'frontend mobile capability adapter');
const commands = [...calls]
  .sort(([a], [b]) => a.localeCompare(b))
  .map(([name, calledFrom]) => ({
    name,
    calledFrom: [...new Set(calledFrom)],
    implementation: supported.get(name) ?? 'unimplemented',
  }));
const report = {
  scope:
    'Static shared IPC and runtime adapter command coverage; excludes event subscriptions and dynamic command names.',
  total: commands.length,
  wired: commands.filter((x) => x.implementation !== 'unimplemented').length,
  missing: commands.filter((x) => x.implementation === 'unimplemented').length,
  commands,
};
const reportPath = path.join(root, 'harmony/ipc-audit.json');
const serialized = JSON.stringify(report, null, 2) + '\n';
if (process.argv.includes('--check')) {
  if (fs.readFileSync(reportPath, 'utf8') !== serialized) {
    throw new Error('Harmony IPC audit is stale; run node scripts/audit-harmony-ipc.mjs');
  }
} else {
  fs.writeFileSync(reportPath, serialized);
}
console.log(
  `Harmony IPC: ${report.wired}/${report.total} wired; ${report.missing} unimplemented. Report: harmony/ipc-audit.json`,
);
