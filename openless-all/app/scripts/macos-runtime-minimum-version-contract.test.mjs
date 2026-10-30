import { readFile } from 'node:fs/promises';
import assert from 'node:assert/strict';

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

const plist = await readFile(new URL('../src-tauri/Info.plist', import.meta.url), 'utf8');
const tap = await readFile(
  new URL('../src-tauri/src/playback_reference_macos.m', import.meta.url),
  'utf8',
);
const manifest = await readFile(new URL('../src-tauri/Cargo.toml', import.meta.url), 'utf8');
assert.match(plist, /<key>NSAudioCaptureUsageDescription<\/key>\s*<string>[^<]+<\/string>/);
assert.match(tap, /@available\(macOS 14\.6, \*\)/);
assert.match(tap, /initWithProcesses:/);
assert.match(tap, /setMuteBehavior:CATapUnmuted/);
assert.match(manifest, /cpal = \{ version = "=0\.15\.3", path = "vendor\/cpal" \}/);
