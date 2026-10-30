import assert from 'node:assert/strict';
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
import { basename, dirname, join, resolve } from 'node:path';
import vm from 'node:vm';
import { fileURLToPath } from 'node:url';
import { load } from 'js-yaml';

const workflow = (name) =>
  load(readFileSync(new URL(`../../../.github/workflows/${name}`, import.meta.url), 'utf8'));
const python = process.platform === 'win32' ? 'python' : 'python3';
const ci = workflow('ci.yml');
const linux = workflow('check-linux-egui.yml');
const harmony = workflow('release-harmony.yml');
const androidSteps = ci.jobs['android-check'].steps;
const runtimeDependencies = androidSteps.find(
  (step) => step.name === 'Install Linux check dependencies',
);
assert.match(runtimeDependencies.run, /\blibpulse0\b/, 'the emulator requires libpulse.so.0');
assert.ok(
  androidSteps.indexOf(runtimeDependencies) <
    androidSteps.findIndex(
      (step) => step.name === 'Prepare the pinned Android emulator and system image',
    ),
);
const install = androidSteps.find(
  (step) => step.name === 'Prepare the pinned Android emulator and system image',
).run;
const boot = androidSteps.find(
  (step) => step.name === 'Run Android Keystore instrumentation tests',
).run;
const coreTests = linux.jobs['test-linux-egui'].steps.find(
  (step) => step.name === 'Test Core, Linux host, and platform boundaries',
).run;
const hotkey =
  'set -euo pipefail\n' +
  coreTests.slice(
    coreTests.indexOf('cargo test --locked -p openless-core --lib'),
    coreTests.indexOf('cargo test --locked -p openless-harmony-host'),
  );
const publish = linux.jobs['publish-linux-egui'].steps.find((step) =>
  step.run?.includes('gh release upload'),
).run;
for (const script of [install, boot, hotkey, publish]) {
  const result = spawnSync('bash', ['-n'], { input: script, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
}

const scratch = mkdtempSync(join(tmpdir(), 'openless-ci-behavior-'));
assert.equal(dirname(resolve(scratch)), resolve(tmpdir()));
assert.ok(basename(scratch).startsWith('openless-ci-behavior-'));
const shellPath = (path) =>
  process.platform === 'win32'
    ? path.replaceAll('\\', '/').replace(/^([a-z]):/i, (_, drive) => '/' + drive.toLowerCase())
    : path;
const executable = (path, source) => {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, '#!/usr/bin/env bash\nset -eu\n' + source);
  chmodSync(path, 0o755);
};
const emulatorSource = `
if [ "\${1:-}" = '-version' ]; then
  echo "Android emulator version 37.3.4.0 (build_id \${FAKE_EMULATOR_BUILD:-16555280})"
  if [ "\${FAKE_VERSION_DETAILS:-0}" = 1 ]; then
    "$TEST_SLEEP" 0.02
    printf '%8192s\n' 'emulator license details'
  fi
  exit "\${FAKE_VERSION_EXIT:-0}"
fi
[ -s "$ANDROID_AVD_HOME/openless-ci.ini" ] || { echo 'Unknown AVD name [openless-ci]' >&2; exit 1; }
echo "$$" > "$RUNNER_TEMP/emulator.pid"
echo 'emulator started'
trap 'echo stopped >> "$RUNNER_TEMP/trace"; exit 0' TERM
while :; do "$TEST_SLEEP" 0.02; done
`;
const stubs = `
name=$(basename "$0")
printf '%s %s\\n' "$name" "$*" >> "$RUNNER_TEMP/trace"
case "$name" in
  curl)
    count=0; [ ! -f "$RUNNER_TEMP/curl-count" ] || count=$(cat "$RUNNER_TEMP/curl-count")
    count=$((count + 1)); echo "$count" > "$RUNNER_TEMP/curl-count"
    [ "$count" -gt "\${FAKE_CURL_FAILURES:-0}" ] || exit 22
    while [ "$#" -gt 0 ]; do
      if [ "$1" = '-o' ]; then printf archive > "$2"; break; fi
      shift
    done
    ;;
  unzip)
    if [ "$1" = '-tq' ]; then
      [ "$(cat "$RUNNER_TEMP/curl-count")" -gt "\${FAKE_BAD_ZIPS:-0}" ] || exit 1
    else
      mkdir -p "$4/emulator"
      cp "$FAKE_EMULATOR_SOURCE" "$4/emulator/emulator"
      echo Pkg.Revision=37.3.4 > "$4/emulator/source.properties"
    fi
    ;;
  sdkmanager)
    [ -s "$ANDROID_HOME/emulator/package.xml" ] || { echo 'SDK would download an unregistered emulator dependency' >&2; exit 1; }
    count=0; [ ! -f "$RUNNER_TEMP/sdk-count" ] || count=$(cat "$RUNNER_TEMP/sdk-count")
    count=$((count + 1)); echo "$count" > "$RUNNER_TEMP/sdk-count"
    [ "$count" -gt "\${FAKE_SDK_FAILURES:-0}" ] || exit 1
    image="$ANDROID_HOME/system-images/android-35/google_apis/x86_64"
    mkdir -p "$image"; echo package > "$image/package.xml"
    if [ "\${FAKE_INCOMPLETE_IMAGE:-0}" != 1 ]; then echo system > "$image/system.img"; fi
    ;;
  sed)
    if [ "$1" = '-i' ]; then echo 'BSD sed requires a backup suffix for -i' >&2; exit 1; fi
    exec "$TEST_SED" "$@"
    ;;
  python3) exec "$TEST_PYTHON" "$@" ;;
  sleep) "$TEST_SLEEP" 0.02 ;;
  avdmanager)
    [ -n "$ANDROID_AVD_HOME" ] || { echo 'SDK tools and emulator require an explicit shared AVD registry' >&2; exit 1; }
    [ -s "$ANDROID_HOME/emulator/package.xml" ] || { echo 'Error: "emulator" package must be installed!' >&2; exit 1; }
    while [ "$1" != '--path' ]; do shift; done
    mkdir -p "$2"; echo hw.keyboard=yes > "$2/config.ini"
    printf 'path=%s\n' "$2" > "$ANDROID_AVD_HOME/openless-ci.ini"
    ;;
  timeout)
    if [[ "$1" == --kill-after=* ]]; then shift; fi
    if [ "$1" = 10s ]; then shift; exec "$@"; fi
    [ "$1" = 600s ] || exit 99
    until [ -s "$RUNNER_TEMP/emulator.pid" ]; do "$TEST_SLEEP" 0.01; done
    if [ "\${FAKE_BOOT_TIMEOUT:-0}" = 1 ]; then exit 124; fi
    shift; exec "$@"
    ;;
  adb)
    case "$*" in
      *getprop*) echo 1 ;;
      *'emu kill'*) kill -TERM "$(cat "$RUNNER_TEMP/emulator.pid")" 2>/dev/null || true ;;
    esac
    ;;
  cargo)
    [ "\${FAKE_CARGO_EXIT:-0}" = 0 ] || exit "$FAKE_CARGO_EXIT"
    if [ "\${FAKE_HOTKEY_MATCH:-1}" = 1 ]; then
      echo 'test api::tests::shared_hotkey_edges_own_hold_auto_and_combo_abort_semantics ... ok'
    else
      echo 'test result: ok. 0 passed; 1381 filtered out'
    fi
    ;;
  gh) [ "$GH_REPO" = 'example/openless' ] ;;
esac
`;
function fixture(name) {
  const dir = join(scratch, name);
  const bin = join(dir, 'bin');
  mkdirSync(bin, { recursive: true });
  for (const command of [
    'curl',
    'unzip',
    'sdkmanager',
    'sleep',
    'python3',
    'sed',
    'avdmanager',
    'timeout',
    'adb',
    'cargo',
    'gh',
  ])
    executable(join(bin, command), stubs);
  executable(join(dir, 'emulator-source'), emulatorSource);
  executable(
    join(dir, 'src-tauri/gen/android/gradlew'),
    'echo "gradle $*" >> "$RUNNER_TEMP/trace"\nexit "${FAKE_GRADLE_EXIT:-0}"\n',
  );
  const sdk = join(dir, 'sdk');
  mkdirSync(sdk);
  const env = {
    ...process.env,
    TEST_BIN: shellPath(bin),
    TEST_PYTHON_COMMAND: python,
    RUNNER_TEMP: shellPath(dir),
    ANDROID_HOME: shellPath(sdk),
    ANDROID_AVD_HOME: '',
    FAKE_EMULATOR_SOURCE: shellPath(join(dir, 'emulator-source')),
    EMULATOR_CACHE_HIT: 'false',
    GH_REPO: 'example/openless',
    RELEASE_TAG: 'v2.0.2-tauri',
  };
  const run = (script, extra = {}) => {
    const guarded =
      'export TEST_SLEEP="$(command -v sleep)" TEST_SED="$(command -v sed)" TEST_PYTHON="$(command -v "$TEST_PYTHON_COMMAND")"\n' +
      'export PATH="$TEST_BIN:$PATH"\n[ "$(command -v curl)" = "$TEST_BIN/curl" ] || exit 99\n' +
      script;
    const result = spawnSync('bash', ['-c', guarded], {
      cwd: dir,
      env: { ...env, ...extra },
      encoding: 'utf8',
      timeout: 15000,
    });
    assert.ifError(result.error, result.stdout + result.stderr);
    return result;
  };
  const trace = () =>
    existsSync(join(dir, 'trace')) ? readFileSync(join(dir, 'trace'), 'utf8') : '';
  const count = (command) =>
    trace()
      .split('\n')
      .filter((line) => line.startsWith(command + ' ')).length;
  const seed = () => {
    executable(join(sdk, 'emulator/emulator'), emulatorSource);
    writeFileSync(join(sdk, 'emulator/source.properties'), 'Pkg.Revision=37.3.4\n');
    const image = join(sdk, 'system-images/android-35/google_apis/x86_64');
    mkdirSync(image, { recursive: true });
    writeFileSync(join(image, 'system.img'), 'system');
    writeFileSync(join(image, 'package.xml'), 'package');
  };
  return { dir, sdk, run, trace, count, seed };
}
try {
  for (const [name, env, expected, curls, sdkCalls] of [
    ['cold', {}, 0, 1, 1],
    ['verbose-version', { FAKE_VERSION_DETAILS: '1' }, 0, 1, 1],
    ['download-retry', { FAKE_CURL_FAILURES: '2' }, 0, 3, 1],
    ['bad-zip-retry', { FAKE_BAD_ZIPS: '2' }, 0, 3, 1],
    ['sdk-retry', { FAKE_SDK_FAILURES: '2' }, 0, 3, 3],
    ['download-failure', { FAKE_CURL_FAILURES: '3' }, 1, 3, 0],
    ['incomplete-image', { FAKE_INCOMPLETE_IMAGE: '1' }, 1, 3, 3],
  ]) {
    const test = fixture(name);
    const result = test.run(install, env);
    assert.equal(result.status, expected, name + ': ' + result.stdout + result.stderr);
    assert.equal(test.count('curl'), curls, name);
    assert.equal(test.count('sdkmanager'), sdkCalls, name);
    assert.equal(existsSync(join(test.sdk, 'emulator/emulator')), expected === 0, name);
    assert.equal(
      existsSync(join(test.sdk, 'emulator/package.xml')),
      expected === 0,
      name + ': emulator must be registered in the SDK',
    );
  }
  for (const [name, env, removeImage, expected] of [
    ['warm', {}, false, 0],
    ['wrong-build', { FAKE_EMULATOR_BUILD: 'wrong' }, false, 1],
    ['version-command-failure', { FAKE_VERSION_EXIT: '3' }, false, 1],
    ['missing-image', {}, true, 1],
  ]) {
    const test = fixture(name);
    test.seed();
    if (removeImage)
      rmSync(join(test.sdk, 'system-images/android-35/google_apis/x86_64/system.img'));
    const result = test.run(install, { EMULATOR_CACHE_HIT: 'true', ...env });
    assert.equal(result.status, expected, result.stdout + result.stderr);
    assert.equal(test.count('curl'), 0, name + ' must not download on a cache hit');
    assert.equal(test.count('sdkmanager'), 0, name);
  }
  for (const [name, env, expected] of [
    ['boot-success', {}, 0],
    ['boot-timeout', { FAKE_BOOT_TIMEOUT: '1' }, 124],
    ['gradle-failure', { FAKE_GRADLE_EXIT: '17' }, 17],
  ]) {
    const test = fixture(name);
    test.seed();
    assert.equal(test.run(install, { EMULATOR_CACHE_HIT: 'true' }).status, 0);
    const result = test.run(boot, env);
    assert.equal(result.status, expected, name + ': ' + result.stdout + result.stderr);
    assert.match(test.trace(), /adb -s emulator-5554 emu kill/, name);
    assert.match(test.trace(), /stopped/, name + ': owned emulator must exit');
    assert.equal(test.count('gradle'), name === 'boot-timeout' ? 0 : 1, name);
    const registry = readFileSync(join(test.dir, 'openless-ci-avds/openless-ci.ini'), 'utf8');
    assert.ok(registry.includes('path=' + shellPath(join(test.dir, 'openless-ci.avd'))));
    const config = readFileSync(join(test.dir, 'openless-ci.avd/config.ini'), 'utf8');
    assert.match(config, /^hw.keyboard=no$/m);
    assert.doesNotMatch(config, /hw.keyboard=yes/);
    if (expected) assert.match(result.stderr, /emulator started/, 'preserve failure log');
    if (name !== 'boot-timeout')
      assert.match(
        test.trace(),
        /gradle .*:app:connectedX86_64DebugAndroidTest -x :app:rustBuildX86_64Debug --no-daemon/,
      );
  }
  for (const [name, env, expected] of [
    ['hotkey-match', {}, 0],
    ['hotkey-missing', { FAKE_HOTKEY_MATCH: '0' }, 1],
    ['hotkey-cargo-failure', { FAKE_CARGO_EXIT: '23' }, 23],
  ]) {
    const test = fixture(name);
    assert.equal(test.run(hotkey, env).status, expected, name);
  }
  {
    const test = fixture('publish-assets');
    const packages = join(test.dir, 'openless-linux-egui-packages');
    const aliases = join(test.dir, 'openless-linux-egui-direct-download');
    for (const [dir, names] of [
      [
        packages,
        ['OpenLess-Linux-egui-test.deb', 'OpenLess-Linux-egui-test.rpm', 'SHA256SUMS-linux'],
      ],
      [aliases, ['OpenLess_linux_x86_64.deb', 'OpenLess_linux_x86_64.rpm']],
    ]) {
      mkdirSync(dir);
      for (const name of names) writeFileSync(join(dir, name), 'fake verified asset');
    }
    assert.equal(test.run(publish).status, 0);
    const upload = test
      .trace()
      .split('\n')
      .find((line) => line.startsWith('gh release upload'));
    for (const asset of [
      'OpenLess-Linux-egui-test.deb',
      'OpenLess-Linux-egui-test.rpm',
      'SHA256SUMS-linux',
      'OpenLess_linux_x86_64.deb',
      'OpenLess_linux_x86_64.rpm',
    ])
      assert.ok(upload.includes(asset));
    assert.ok(upload.endsWith('--clobber'));
    rmSync(join(aliases, 'OpenLess_linux_x86_64.rpm'));
    const uploadsBefore = test
      .trace()
      .split('\n')
      .filter((line) => line.startsWith('gh release upload')).length;
    assert.equal(test.run(publish).status, 1, 'missing asset must not publish');
    assert.equal(
      test
        .trace()
        .split('\n')
        .filter((line) => line.startsWith('gh release upload')).length,
      uploadsBefore,
    );
  }
  {
    const test = fixture('signing-passwords');
    const signing = join(test.dir, 'signing');
    const output = join(test.dir, 'output');
    mkdirSync(signing);
    mkdirSync(output);
    for (const channel of ['release', 'test'])
      for (const suffix of ['cer', 'p7b', 'p12'])
        writeFileSync(join(signing, channel + '.' + suffix), 'fake');
    writeFileSync(join(signing, 'keystore-password'), 'local-file-store');
    writeFileSync(join(signing, 'key-password'), 'local-file-key');
    writeFileSync(join(output, 'entry-default-unsigned.hap'), 'fake');
    const jar = join(test.sdk, 'sdk/default/openharmony/toolchains/lib/hap-sign-tool.jar');
    mkdirSync(dirname(jar), { recursive: true });
    writeFileSync(jar, 'fake');
    const script = fileURLToPath(new URL('./sign-harmony-hap.py', import.meta.url));
    const source = `
import contextlib, io, json, os, runpy, sys
from types import SimpleNamespace
from unittest.mock import patch
script, toolchain, signing, output, channel, values = sys.argv[1:]
with patch.dict(os.environ, json.loads(values), clear=True), patch('sys.argv', [script, toolchain, signing, output, '--prefix', channel]), patch('subprocess.run', return_value=SimpleNamespace(returncode=0, stdout='', stderr='')) as run, contextlib.redirect_stdout(io.StringIO()):
    runpy.run_path(script, run_name='__main__')
    command = run.call_args.args[0]
print(json.dumps([command[command.index('-keystorePwd') + 1], command[command.index('-keyPwd') + 1]]))
`;
    const runSign = (channel, env) => {
      const result = spawnSync(
        python,
        ['-B', '-c', source, script, test.sdk, signing, output, channel, JSON.stringify(env)],
        { encoding: 'utf8' },
      );
      assert.equal(result.status, 0, result.stderr);
      return JSON.parse(result.stdout);
    };
    for (const channel of ['release', 'test']) {
      const secrets = {
        HARMONY_KEYSTORE_PASSWORD: 'old-store',
        HARMONY_KEY_PASSWORD: 'old-key',
        HARMONY_RELEASE_KEYSTORE_PASSWORD: 'release-store',
        HARMONY_RELEASE_KEY_PASSWORD: 'release-key',
        HARMONY_TEST_KEYSTORE_PASSWORD: 'test-store',
        HARMONY_TEST_KEY_PASSWORD: 'test-key',
      };
      const env = Object.fromEntries(
        ['HARMONY_KEYSTORE_PASSWORD', 'HARMONY_KEY_PASSWORD'].map((name) => [
          name,
          vm.runInNewContext(harmony.jobs['build-harmony-hap'].env[name].slice(3, -2), {
            inputs: { channel },
            secrets,
          }),
        ]),
      );
      assert.deepEqual(runSign(channel, env), [channel + '-store', channel + '-key']);
    }
    assert.deepEqual(runSign('release', {}), ['local-file-store', 'local-file-key']);
    rmSync(join(signing, 'key-password'));
    assert.deepEqual(runSign('release', { HARMONY_KEYSTORE_PASSWORD: 'resolved-store' }), [
      'resolved-store',
      'resolved-store',
    ]);
  }
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
console.log(
  'ci-workflow-behavior: install retries/cache hits, emulator cleanup, exact-test guard, assets and signing passwords passed',
);
