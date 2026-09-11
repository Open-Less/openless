import { readFile, readdir } from 'node:fs/promises';
import { extname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const appRoot = new URL('..', import.meta.url);
const roots = [
  new URL('../linux-egui', import.meta.url),
  new URL('../scripts/package-linux-egui.sh', import.meta.url),
  new URL('../../../.github/workflows/release-linux-egui.yml', import.meta.url),
];
const sourceExtensions = new Set(['.rs', '.toml', '.sh', '.yml', '.yaml']);

async function collect(url) {
  const path = fileURLToPath(url);
  if (extname(path)) return [path];
  const entries = await readdir(path, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    if (entry.name === 'target') continue;
    const child = join(path, entry.name);
    if (entry.isDirectory()) files.push(...await collect(pathToFileURL(`${child}/`)));
    else if (sourceExtensions.has(extname(entry.name))) files.push(child);
  }
  return files;
}

const files = (await Promise.all(roots.map(collect))).flat();
const violations = [];
for (const file of files) {
  const source = await readFile(file, 'utf8');
  // The Remote Input TLS identity implementation remains one shared source file
  // while its platform-neutral extraction is tracked separately. It is included
  // directly by the Linux host and does not introduce a Cargo dependency on the
  // Tauri crate, runtime, WebKit, or Qwen ASR.
  const allowsSharedTlsIdentity = file.endsWith('/linux-egui/src/remote_input.rs');
  if (!allowsSharedTlsIdentity && source.includes('src-tauri')) violations.push(`${file}: references src-tauri`);
  if (file.endsWith('Cargo.toml') && /^\s*(tauri|wry|webkit\w*)\s*=/mi.test(source)) {
    violations.push(`${file}: declares a Tauri/Wry/WebKit dependency`);
  }
  if (/\b(Mock|Fake)(Backend|Provider|Repository)\b/.test(source)) {
    violations.push(`${file}: production source names a mock backend/provider/repository`);
  }
}

if (violations.length) {
  throw new Error(`Linux egui Tauri-free contract failed:\n${violations.join('\n')}`);
}

console.log(`linux-egui-tauri-free-contract.test.mjs passed (${files.length} files)`);
