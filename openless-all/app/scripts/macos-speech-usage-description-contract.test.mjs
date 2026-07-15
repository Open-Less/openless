import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

if (process.platform !== 'darwin') {
  console.log('macOS speech usage description contract skipped on non-macOS');
  process.exit(0);
}

const scriptsDir = fileURLToPath(new URL('.', import.meta.url));
const checker = join(scriptsDir, 'check-macos-speech-usage-description.sh');
const fixtureDir = mkdtempSync(join(tmpdir(), 'openless-speech-usage-'));

function plist(value) {
  return `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>${value}</dict></plist>
`;
}

function writeFixture(name, value) {
  const path = join(fixtureDir, name);
  writeFileSync(path, plist(value));
  return path;
}

function runChecker(path, env = {}) {
  return spawnSync('bash', [checker, path], {
    encoding: 'utf8',
    env: { ...process.env, ...env },
  });
}

function expectSuccessSilent(name, path) {
  const result = runChecker(path);
  if (result.status !== 0) {
    throw new Error(`${name}: expected exit 0, got ${result.status}\n${result.stderr}`);
  }
  if (result.stdout !== '' || result.stderr !== '') {
    throw new Error(`${name}: success must be silent, got ${JSON.stringify(result.stdout + result.stderr)}`);
  }
}

function expectFailure(name, path, env = {}) {
  const result = runChecker(path, env);
  if (result.status === 0) {
    throw new Error(`${name}: expected non-zero exit`);
  }
}

try {
  expectSuccessSilent(
    'valid string',
    writeFixture(
      'valid.plist',
      '<key>NSSpeechRecognitionUsageDescription</key><string>OpenLess transcribes speech locally.</string>',
    ),
  );
  expectFailure('missing key', writeFixture('missing-key.plist', ''));
  expectFailure(
    'empty string',
    writeFixture('empty-string.plist', '<key>NSSpeechRecognitionUsageDescription</key><string></string>'),
  );
  expectFailure(
    'whitespace-only string',
    writeFixture(
      'whitespace-string.plist',
      '<key>NSSpeechRecognitionUsageDescription</key><string>  \n\t  </string>',
    ),
  );
  for (const [name, slug, whitespace] of [
    ['NBSP', 'nbsp', '\u00a0'],
    ['EM SPACE', 'em-space', '\u2003'],
    ['IDEOGRAPHIC SPACE', 'ideographic-space', '\u3000'],
  ]) {
    expectFailure(
      `${name}-only string under C locale`,
      writeFixture(
        `${slug}-only.plist`,
        `<key>NSSpeechRecognitionUsageDescription</key><string>${whitespace}</string>`,
      ),
      { LC_ALL: 'C', LANG: 'C' },
    );
  }
  expectFailure(
    'wrong type',
    writeFixture('wrong-type.plist', '<key>NSSpeechRecognitionUsageDescription</key><true/>'),
  );
  expectFailure('missing file', join(fixtureDir, 'does-not-exist.plist'));
} finally {
  rmSync(fixtureDir, { recursive: true, force: true });
}
