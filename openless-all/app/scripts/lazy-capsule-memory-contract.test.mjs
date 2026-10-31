import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const base = JSON.parse(await readFile(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf8'));
const capsule = base.app.windows.find((window) => window.label === 'capsule');
assert.equal(capsule?.create, false, 'macOS capsule WebView must not be created at startup');
const resources = await readFile(new URL('../src-tauri/src/desktop_resources.rs', import.meta.url), 'utf8');
const host = await readFile(new URL('../src-tauri/src/tauri_coordinator_host.rs', import.meta.url), 'utf8');
assert.match(resources, /ensure_capsule_window[\s\S]*?WebviewWindowBuilder::from_config/);
assert.match(host, /fn window\(&self\)[\s\S]*?ensure_capsule_window/);
console.log('lazy capsule creation contract passed');
