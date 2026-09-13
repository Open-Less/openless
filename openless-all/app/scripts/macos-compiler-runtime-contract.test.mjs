import { readFile } from 'node:fs/promises';

const buildScript = await readFile(new URL('../src-tauri/build.rs', import.meta.url), 'utf8');
const requiredFragments = ['-print-resource-dir', 'rustc-link-search=native=', 'clang_rt.osx'];

for (const fragment of requiredFragments) {
  if (!buildScript.includes(fragment)) {
    throw new Error(`macOS compiler runtime link contract is missing: ${fragment}`);
  }
}

console.log('macOS compiler runtime link contract passed');
