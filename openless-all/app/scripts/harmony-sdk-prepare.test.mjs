// Fixture test for `prepare-harmony-sdk.sh`: the CI toolchain must be an API 26
// (HarmonyOS 7) tree and must be rejected from its own metadata rather than from
// a tools build number. Every case runs the real script against small synthetic
// archives; no 2.28 GB download, no network, no real toolchain touched.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  chmodSync,
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

// `prepare-harmony-sdk.sh` is a POSIX shell installer (bash/tar/grep). Windows
// has no bash in the hosted test image, so this fixture is skipped there; the
// Ubuntu Harmony job runs it unconditionally.
if (process.platform === 'win32') {
  console.log('HarmonyOS SDK prepare: skipped on win32 (POSIX installer fixture)');
  process.exit(0);
}

const appRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const script = resolve(appRoot, '../harmony/scripts/prepare-harmony-sdk.sh');
const text = readFileSync(script, 'utf8');

// The pinned default is the API 26 tools archive; it must stay a fixed,
// sharded public asset with a complete digest - never `latest`.
assert.match(
  text,
  /DEFAULT_PARTS="https:\/\/[^"]*26\.0\.0\.821\/ohos-sdk-linux-amd64\.tar\.gz\.aa https:\/\/[^"]*26\.0\.0\.821\/ohos-sdk-linux-amd64\.tar\.gz\.ab"/,
  'default download must pin both API 26 shards by release tag',
);
assert.match(text, /DEFAULT_SHA256=[0-9a-f]{64}/, 'the complete archive digest must be pinned');
assert.doesNotMatch(
  text,
  /releases\/latest|huggingface\.co/,
  'no floating or API 21 mirror default',
);
assert.match(text, /HARMONY_MIN_API:-26/, 'minimum API must default to 26');
// The API level comes from sdk-pkg.json, never from the archive/tools name.
assert.match(text, /sdk_metadata "\$metadata" apiVersion/);
assert.match(text, /below the required API \$min_api/);

const work = mkdtempSync(join(tmpdir(), 'openless-sdk-prepare-'));
const home = join(work, 'home');
const temp = join(work, 'tmp');
mkdirSync(home, { recursive: true });
mkdirSync(temp, { recursive: true });

function makeTree(api, stage = 'Release') {
  const dir = mkdtempSync(join(work, 'tree-'));
  for (const part of [
    'sdk/default/openharmony/ets',
    'sdk/default/openharmony/toolchains',
    'hvigor',
    'tool',
    'ohpm',
    'bin',
  ]) {
    mkdirSync(join(dir, part), { recursive: true });
  }
  writeFileSync(
    join(dir, 'sdk/default/sdk-pkg.json'),
    JSON.stringify({
      meta: {},
      data: {
        apiVersion: String(api),
        displayName: `HarmonyOS ${api}.0.0`,
        platformVersion: `${api}.0.0`,
        stage,
      },
    }),
  );
  writeFileSync(join(dir, 'bin/hvigorw'), '#!/bin/sh\nexit 0\n');
  chmodSync(join(dir, 'bin/hvigorw'), 0o755);
  writeFileSync(join(dir, 'LICENSE.txt'), 'fixture license');
  return dir;
}

function pack(tree, name) {
  const out = join(work, name);
  execFileSync('tar', ['-czf', out, '-C', tree, '.']);
  return out;
}

function digest(file) {
  return createHash('sha256').update(readFileSync(file)).digest('hex');
}

function run({ url, parts, sha256, minApi, replace, toolchain }) {
  const env = { ...process.env };
  delete env.HARMONY_SDK_URL;
  delete env.HARMONY_SDK_PARTS;
  delete env.HARMONY_SDK_SHA256;
  delete env.HARMONY_MIN_API;
  delete env.HARMONY_SDK_REPLACE;
  env.HOME = home;
  env.TMPDIR = temp;
  env.OPENLESS_HARMONY_TOOLCHAIN = toolchain;
  if (url !== undefined) env.HARMONY_SDK_URL = url;
  if (parts !== undefined) env.HARMONY_SDK_PARTS = parts;
  if (sha256 !== undefined) env.HARMONY_SDK_SHA256 = sha256;
  if (minApi !== undefined) env.HARMONY_MIN_API = minApi;
  if (replace !== undefined) env.HARMONY_SDK_REPLACE = replace;
  try {
    const out = execFileSync('bash', [script], {
      env,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    return { code: 0, output: out };
  } catch (error) {
    return { code: error.status ?? 1, output: `${error.stdout ?? ''}${error.stderr ?? ''}` };
  }
}

const fresh = () => mkdtempSync(join(work, 'toolchain-'));

async function main() {
  const api21 = pack(makeTree(21), 'api21.tar.gz');
  const api26 = pack(makeTree(26), 'api26.tar.gz');
  const api26Sha = digest(api26);

  // 1. An API 21 archive is the old CI toolchain; it must fail before ArkTS.
  {
    const result = run({ url: `file://${api21}`, sha256: digest(api21), toolchain: fresh() });
    assert.notEqual(result.code, 0, 'API 21 archive must be rejected');
    assert.match(result.output, /below the required API 26/);
  }

  // 2. The same archive passes when a lower floor is requested explicitly, so the
  //    guard is metadata-driven and configurable, not a hard-coded path check.
  {
    const result = run({
      url: `file://${api21}`,
      sha256: digest(api21),
      minApi: '21',
      toolchain: fresh(),
    });
    assert.equal(result.code, 0, result.output);
    assert.match(result.output, /SDK ready at .* \(API 21, Release\)/);
  }

  // 3. API 26 installs and keeps the toolchain shape the build script drives.
  {
    const root = fresh();
    const result = run({ url: `file://${api26}`, sha256: api26Sha, toolchain: root });
    assert.equal(result.code, 0, result.output);
    assert.match(result.output, /SDK ready at .* \(API 26, Release\)/);
    assert.ok(existsSync(join(root, 'bin/hvigorw')), 'bundled hvigorw must survive install');
    const installed = JSON.parse(readFileSync(join(root, 'sdk/default/sdk-pkg.json'), 'utf8'));
    assert.equal(String(installed.data.apiVersion), '26');

    // 4. A second run reuses the compatible tree instead of re-downloading.
    const reused = run({ url: `file://${api26}`, sha256: api26Sha, toolchain: root });
    assert.equal(reused.code, 0, reused.output);
    assert.match(reused.output, /already present at .* \(API 26\)/);
  }

  // 5. A checksum mismatch aborts before anything is extracted.
  {
    const root = fresh();
    const result = run({ url: `file://${api26}`, sha256: '0'.repeat(64), toolchain: root });
    assert.notEqual(result.code, 0);
    assert.match(result.output, /checksum mismatch/);
    assert.ok(!existsSync(join(root, 'sdk/default/sdk-pkg.json')));
  }

  // 6. Sharded archives are concatenated in order before the digest is checked.
  {
    const bytes = readFileSync(api26);
    const cut = Math.floor(bytes.length / 2);
    const head = join(work, 'api26-split.tar.gz.aa');
    const tail = join(work, 'api26-split.tar.gz.ab');
    writeFileSync(head, bytes.subarray(0, cut));
    writeFileSync(tail, bytes.subarray(cut));
    const root = fresh();
    const result = run({
      parts: `file://${head} file://${tail}`,
      sha256: api26Sha,
      toolchain: root,
    });
    assert.equal(result.code, 0, result.output);
    assert.match(result.output, /SDK ready at .* \(API 26, Release\)/);
    assert.ok(existsSync(join(root, 'sdk/default/openharmony/ets')));
  }

  // 7. A stale API 21 tree on disk is refused unless replacement is explicit.
  {
    const root = fresh();
    const old = makeTree(21);
    cpSync(old, root, { recursive: true });
    const refused = run({ url: `file://${api26}`, sha256: api26Sha, toolchain: root });
    assert.notEqual(refused.code, 0);
    assert.match(refused.output, /below the required 26/);
    assert.match(refused.output, /HARMONY_SDK_REPLACE=1/);
    const replaced = run({
      url: `file://${api26}`,
      sha256: api26Sha,
      replace: '1',
      toolchain: root,
    });
    assert.equal(replaced.code, 0, replaced.output);
    const api = JSON.parse(readFileSync(join(root, 'sdk/default/sdk-pkg.json'), 'utf8'));
    assert.equal(String(api.data.apiVersion), '26');
  }

  // 8. Only a Release SDK may provide the pinned toolchain.
  {
    const beta = pack(makeTree(26, 'Beta1'), 'api26-beta.tar.gz');
    const root = fresh();
    const result = run({ url: `file://${beta}`, sha256: digest(beta), toolchain: root });
    assert.notEqual(result.code, 0);
    assert.match(result.output, /not Release/);
    assert.ok(
      !existsSync(join(root, 'sdk/default/sdk-pkg.json')),
      'a rejected SDK installs nothing',
    );
  }

  // 9. A rejected candidate never damages an existing compatible install, even
  //    when replacement was explicitly allowed: validation happens while staged.
  {
    const root = fresh();
    const installed = run({ url: `file://${api26}`, sha256: api26Sha, toolchain: root });
    assert.equal(installed.code, 0, installed.output);
    const marker = join(root, 'keep-me.txt');
    writeFileSync(marker, 'previous toolchain');
    // Drop the bundled hvigorw so the reuse shortcut cannot mask the download
    // path this case must exercise.
    rmSync(join(root, 'bin/hvigorw'), { force: true });
    const beta = pack(makeTree(26, 'Canary'), 'api26-canary.tar.gz');
    const refused = run({
      url: `file://${beta}`,
      sha256: digest(beta),
      replace: '1',
      toolchain: root,
    });
    assert.notEqual(refused.code, 0, 'non-Release candidate must be rejected');
    assert.match(refused.output, /not Release/);
    assert.ok(existsSync(marker), 'existing toolchain files must survive a failed install');
    const still = JSON.parse(readFileSync(join(root, 'sdk/default/sdk-pkg.json'), 'utf8'));
    assert.equal(String(still.data.apiVersion), '26');
    assert.equal(still.data.stage, 'Release');
  }

  // 10. A non-numeric floor fails loudly instead of reaching `[ -lt ]`.
  {
    const result = run({
      url: `file://${api26}`,
      sha256: api26Sha,
      minApi: 'twenty-six',
      toolchain: fresh(),
    });
    assert.notEqual(result.code, 0);
    assert.match(result.output, /HARMONY_MIN_API must be a positive integer/);
  }

  // 11. Match the real 26.0.0.821 archive: a command-line-tools wrapper plus
  // emulator/sdk-pkg.json. The emulator metadata is not an SDK API contract.
  {
    const wrapper = mkdtempSync(join(work, 'wrapped-'));
    const tools = join(wrapper, 'command-line-tools');
    cpSync(makeTree(26), tools, { recursive: true });
    mkdirSync(join(tools, 'emulator'), { recursive: true });
    writeFileSync(
      join(tools, 'emulator/sdk-pkg.json'),
      JSON.stringify({ meta: {}, data: { path: 'emulator', version: '26.0.0.821' } }),
    );
    const archive = pack(wrapper, 'wrapped-tools.tar.gz');
    const root = fresh();
    const result = run({ url: `file://${archive}`, sha256: digest(archive), toolchain: root });
    assert.equal(result.code, 0, result.output);
    const installed = JSON.parse(readFileSync(join(root, 'sdk/default/sdk-pkg.json'), 'utf8'));
    assert.equal(installed.data.apiVersion, '26');
    assert.ok(existsSync(join(root, 'bin/hvigorw')));
  }

  console.log(
    'HarmonyOS SDK prepare: API 26 install, API 21/checksum/non-Release rejection, shard concatenation, staged validation, reuse, explicit replacement and wrapped toolchain root discovery passed',
  );
}

try {
  await main();
} finally {
  rmSync(work, { recursive: true, force: true });
}
