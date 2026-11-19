import { readFile, readdir } from 'node:fs/promises';
import { extname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const appRoot = new URL('..', import.meta.url);
const roots = [
  new URL('../linux-host', import.meta.url),
  new URL('../scripts/package-linux-qt.sh', import.meta.url),
  // Linux 已并入公共工作流：检查它自己的打包 action 与 ci.yml 里的 Linux 作业片段。
  new URL('../../../.github/actions/build-linux-packages/action.yml', import.meta.url),
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
    if (entry.isDirectory()) files.push(...(await collect(pathToFileURL(`${child}/`))));
    else if (sourceExtensions.has(extname(entry.name))) files.push(child);
  }
  return files;
}

const files = (await Promise.all(roots.map(collect))).flat();

// `ci.yml` 里的 Linux 作业会调用 `src-tauri/backend-tests`（独立的 remote TLS
// 测试 crate，不链 Tauri 运行时），所以这份契约只扫 Linux 自己的打包定义。
const violations = [];
for (const file of files) {
  const source = await readFile(file, 'utf8');
  const normalizedFile = file.replaceAll('\\', '/');
  if (source.includes('src-tauri')) violations.push(`${file}: references src-tauri`);
  if (normalizedFile.endsWith('Cargo.toml') && /^\s*(tauri|wry|webkit\w*)\s*=/im.test(source)) {
    violations.push(`${file}: declares a Tauri/Wry/WebKit dependency`);
  }
  if (/\b(Mock|Fake)(Backend|Provider|Repository)\b/.test(source)) {
    violations.push(`${file}: production source names a mock backend/provider/repository`);
  }
}

if (violations.length) {
  throw new Error(`Linux host Tauri-free contract failed:\n${violations.join('\n')}`);
}

console.log(`linux-host-tauri-free-contract.test.mjs passed (${files.length} files)`);
