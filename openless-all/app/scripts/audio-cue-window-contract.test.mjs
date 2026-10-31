import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const app = await readFile(new URL('../src/App.tsx', import.meta.url), 'utf8');
const shell = await readFile(new URL('../src/components/FloatingShell.tsx', import.meta.url), 'utf8');
const listener = await readFile(new URL('../src/components/AudioCue.tsx', import.meta.url), 'utf8');

assert.match(app, /if \(isCapsule\)[\s\S]*?<AudioCueListener \/>/);
assert.doesNotMatch(shell, /AudioCueListener/);
assert.match(listener, /currentWindow\.label !== 'capsule'/);
assert.match(listener, /stopAudioCue\(\);/);
console.log('audio cue is scoped to the resident capsule window');
