import { readFile } from 'node:fs/promises';

const raw = await readFile(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf-8');
const config = JSON.parse(raw);
const minimumSystemVersion = config.bundle?.macOS?.minimumSystemVersion;

if (typeof minimumSystemVersion !== 'string') {
  throw new Error('macOS minimumSystemVersion config is missing');
}

const [major = 0, minor = 0] = minimumSystemVersion.split('.').map(Number);
if (major < 14 || (major === 14 && minor < 0)) {
  throw new Error(
    `macOS minimumSystemVersion must be at least 14.0 for native experimental modules; got ${minimumSystemVersion}`,
  );
}

console.log(`macOS runtime minimum-system-version contract passed: ${minimumSystemVersion}`);
