import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const root = fileURLToPath(new URL('../', import.meta.url));
const ipc = path.join(root, 'src/lib/ipc');
const calls = new Map();
for (const file of fs.readdirSync(ipc).filter((f) => f.endsWith('.ts') && !f.endsWith('.test.ts') && f !== 'mock-data.ts')) {
  const text = fs.readFileSync(path.join(ipc, file), 'utf8');
  for (const match of text.matchAll(/(?:invokeOrMock|invoke)(?:<[^;]*?>)?\(\s*['"]([a-z][a-z0-9_]+)['"]/g)) {
    const name = match[1];
    if (!calls.has(name)) calls.set(name, []);
    calls.get(name).push(`src/lib/ipc/${file}`);
  }
}
const rust = fs.readFileSync(path.join(root, 'harmony/rust/src/lib.rs'), 'utf8');
const supported = new Map();
for (const arm of rust.matchAll(/^\s*((?:"[a-z][a-z0-9_]+"\s*\|\s*)*"[a-z][a-z0-9_]+")\s*=>/gm)) {
  for (const name of arm[1].matchAll(/"([^"]+)"/g)) supported.set(name[1], 'rust');
}
const ark = fs.readFileSync(path.join(root, 'harmony/entry/src/main/ets/HarmonyApi.ets'), 'utf8');
for (const match of ark.matchAll(/case '([a-z][a-z0-9_]+)'\s*:/g)) supported.set(match[1], 'arkts');
for (const name of ['get_windows_ime_status', 'get_hotkey_status', 'get_hotkey_capability']) supported.set(name, 'frontend mobile capability adapter');
const commands = [...calls].sort(([a], [b]) => a.localeCompare(b)).map(([name, calledFrom]) => ({name, calledFrom: [...new Set(calledFrom)], implementation: supported.get(name) ?? 'unimplemented'}));
const report = {scope:'Static shared IPC command coverage; excludes event subscriptions, dynamic commands and desktop-only calls outside src/lib/ipc.', total:commands.length, wired:commands.filter((x)=>x.implementation!=='unimplemented').length, missing:commands.filter((x)=>x.implementation==='unimplemented').length, commands};
fs.writeFileSync(path.join(root, 'harmony/ipc-audit.json'), JSON.stringify(report, null, 2) + '\n');
console.log(`Harmony IPC: ${report.wired}/${report.total} wired; ${report.missing} unimplemented. Report: harmony/ipc-audit.json`);
