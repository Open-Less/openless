import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const app = new URL('../', import.meta.url);
const read = async (path) => (await readFile(new URL(path, app), 'utf8')).replace(/\r\n/g, '\n');
const pkg = JSON.parse(await read('package.json'));
// A release smoke test can pass an exact expected version; ordinary CI ensures
// all consumers agree with the package manifest across release channels.
const version = process.argv[2] ?? pkg.version;
assert.match(
  version,
  /^\d+\.\d+\.\d+(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/,
  'desktop version must be a SemVer string',
);
const lock = JSON.parse(await read('package-lock.json'));
const tauri = JSON.parse(await read('src-tauri/tauri.conf.json'));
const cargo = await read('src-tauri/Cargo.toml');
const cargoLock = await read('src-tauri/Cargo.lock');

for (const actual of [pkg.version, lock.version, lock.packages[''].version, tauri.version]) {
  assert.equal(
    actual,
    version,
    'desktop version must preserve the requested case and build suffix',
  );
}
assert.equal(cargo.match(/^version = "([^"]+)"/m)?.[1], version);
assert.equal(
  cargoLock.match(/\[\[package\]\]\nname = "openless"\nversion = "([^"]+)"/)?.[1],
  version,
);
console.log('desktop-version-contract.test.mjs passed: ' + version);
