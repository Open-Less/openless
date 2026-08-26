import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const appRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const capsule = readFileSync(join(appRoot, 'src/components/Capsule.tsx'), 'utf8');
const english = readFileSync(join(appRoot, 'src/i18n/en.ts'), 'utf8');
const simplifiedChinese = readFileSync(join(appRoot, 'src/i18n/zh-CN.ts'), 'utf8');
const promptStart = capsule.indexOf('function RecordingRecoveryPrompt(');
const promptEnd = capsule.indexOf('// ───────── 经典药丸', promptStart);

assert.notEqual(promptStart, -1, 'recording recovery prompt must exist');
assert.notEqual(promptEnd, -1, 'recording recovery prompt boundary must exist');

const prompt = capsule.slice(promptStart, promptEnd);
const buttonCount = prompt.match(/<button\b/g)?.length ?? 0;

assert.equal(buttonCount, 1, 'recording recovery prompt must expose exactly one button');
assert.match(prompt, /return\s*\(\s*<button\b/);
assert.match(prompt, /onClick=\{\(\) => void resume\(\)\}/);
assert.doesNotMatch(prompt, /capsule\.recovery\.question/);
assert.doesNotMatch(prompt, /role="status"/);
assert.match(english, /continue: 'Continue recording'/);
assert.match(simplifiedChinese, /continue: '继续录音'/);

console.log('recording recovery prompt contract tests passed');
