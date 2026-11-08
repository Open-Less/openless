import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import {
  chmodSync,
  existsSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

// Exercise the production downloader with a curl that times out after writing
// a partial response. Retrying must resume that file and still verify integrity.
const source = readFileSync(
  new URL('../../ios/scripts/bootstrap-local-speech.sh', import.meta.url),
  'utf8',
);
const fetchFunction = source.match(/^fetch\(\) \{[\s\S]*?^\}/m)?.[0];
assert.ok(fetchFunction);
const scratch = mkdtempSync(join(tmpdir(), 'openless-ios-download-'));
try {
  const bin = join(scratch, 'bin');
  mkdirSync(bin);
  const executable = (name, content) => {
    const path = join(bin, name);
    writeFileSync(path, '#!/usr/bin/env bash\nset -eu\n' + content);
    chmodSync(path, 0o755);
  };
  executable('sleep', 'exit 0\n');
  executable(
    'curl',
    `
max=0; speed=0; window=0; resume=''; output=''
while [ "$#" -gt 0 ]; do
  case "$1" in
    --max-time) max="$2"; shift ;;
    --speed-limit) speed="$2"; shift ;;
    --speed-time) window="$2"; shift ;;
    --continue-at) resume="$2"; shift ;;
    -o) output="$2"; shift ;;
  esac
  shift
done
[ "$max" -gt 0 ] && [ "$speed" -gt 0 ] && [ "$window" -gt 0 ] && [ "$resume" = '-' ] || exit 99
count=0; [ ! -f "$TRACE" ] || count=$(cat "$TRACE")
count=$((count + 1)); echo "$count" > "$TRACE"
case "$MODE" in
  timeout) printf partial > "$output"; exit 28 ;;
  resume)
    if [ "$count" = 1 ]; then printf partial > "$output"; exit 28; fi
    [ "$(cat "$output")" = partial ] || exit 99
    ;;
  corrupt) printf corrupt > "$output"; exit 0 ;;
esac
printf verified > "$output"
`,
  );
  const sha = createHash('sha256').update('verified').digest('hex');
  const run = (mode, output) => {
    const result = spawnSync(
      'bash',
      ['-e', '-c', fetchFunction + '\nfetch https://example.invalid/model "$SHA" "$OUTPUT"'],
      {
        env: {
          ...process.env,
          PATH: bin + ':' + process.env.PATH,
          MODE: mode,
          TRACE: join(scratch, mode + '-calls'),
          SHA: sha,
          OUTPUT: output,
        },
        encoding: 'utf8',
        timeout: 5000,
      },
    );
    assert.ifError(result.error);
    return result;
  };
  for (const [mode, expectedCalls] of [
    ['success', 1],
    ['resume', 2],
  ]) {
    const output = join(scratch, mode);
    const result = run(mode, output);
    assert.equal(result.status, 0, result.stdout + result.stderr);
    assert.equal(readFileSync(output, 'utf8'), 'verified');
    assert.ok(!existsSync(output + '.download'));
    assert.equal(Number(readFileSync(join(scratch, mode + '-calls'), 'utf8')), expectedCalls);
    assert.equal(run(mode, output).status, 0);
    assert.equal(Number(readFileSync(join(scratch, mode + '-calls'), 'utf8')), expectedCalls);
  }
  for (const [mode, expectedCalls] of [
    ['timeout', 6],
    ['corrupt', 1],
  ]) {
    const output = join(scratch, mode);
    assert.notEqual(run(mode, output).status, 0);
    assert.ok(!existsSync(output), 'failed downloads must not be promoted to verified files');
    assert.equal(Number(readFileSync(join(scratch, mode + '-calls'), 'utf8')), expectedCalls);
  }
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
console.log('ios-ci-downloads.test.mjs passed');
