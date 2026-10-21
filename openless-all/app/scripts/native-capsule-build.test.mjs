import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';
import yaml from 'js-yaml';

const scriptUrl = new URL('./build-native-capsule.mjs', import.meta.url);
const script = readFileSync(scriptUrl, 'utf8')
  .replace(/^import .*;\r?\n/gm, '')
  .replaceAll('import.meta.url', JSON.stringify(scriptUrl.href));
const root = resolve(dirname(fileURLToPath(scriptUrl)), '..');
function run(env = {}, debug = false) {
  const copies = [];
  const commands = [];
  vm.runInNewContext(script, {
    dirname,
    resolve,
    fileURLToPath,
    console: { log() {} },
    process: { argv: debug ? ['node', 'script', '--debug'] : ['node', 'script'], env },
    spawnSync: (command, args) => {
      commands.push([command, [...args]]);
      return { status: 0, stdout: 'host: x86_64-pc-windows-msvc\n' };
    },
    mkdirSync() {},
    copyFileSync: (source, destination) => copies.push([source, destination]),
  });
  return { copies, commands };
}
const target = 'x86_64-pc-windows-msvc';
{
  const { copies, commands } = run({}, true);
  assert.deepEqual(commands[1][1], [
    'build',
    '--locked',
    '-p',
    'openless-native-capsule',
    '--target',
    target,
  ]);
  assert.deepEqual(
    copies.map(([, destination]) => destination),
    [
      resolve(root, 'src-tauri/binaries', 'openless-native-capsule-' + target + '.exe'),
      resolve(root, 'src-tauri/target/debug/openless-native-capsule.exe'),
      resolve(root, 'src-tauri/target', target, 'debug/openless-native-capsule.exe'),
    ],
  );
}
{
  const targetDir = resolve(root, 'custom-target');
  const { copies, commands } = run({ CARGO_TARGET_DIR: targetDir });
  assert.equal(commands[1][1].at(-1), '--release');
  assert.equal(copies[0][0], resolve(targetDir, target, 'release/openless-native-capsule.exe'));
  assert.equal(copies[1][1], resolve(targetDir, 'release/openless-native-capsule.exe'));
  assert.equal(copies.length, 2, 'never copy the target-specific helper onto itself');
}
{
  const { copies } = run({ TAURI_ENV_TARGET_TRIPLE: 'aarch64-apple-darwin' });
  assert.equal(
    copies[0][1],
    resolve(root, 'src-tauri/binaries/openless-native-capsule-aarch64-apple-darwin'),
  );
}
assert.throws(
  () => run({ TAURI_ENV_TARGET_TRIPLE: 'aarch64-linux-android' }),
  /macOS or Windows target/,
);
const { scripts } = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8'));
for (const platform of ['macos', 'windows']) {
  const config = JSON.parse(
    readFileSync(new URL('../src-tauri/tauri.' + platform + '.conf.json', import.meta.url), 'utf8'),
  );
  for (const hook of [config.build.beforeBuildCommand, config.build.beforeDevCommand]) {
    for (const [, name] of hook.matchAll(/npm run ([\w:-]+)/g))
      assert.ok(scripts[name], platform + ' references missing npm script: ' + name);
  }
  assert.ok(config.bundle.externalBin.includes('binaries/openless-native-capsule'));
}
const ci = yaml.load(
  readFileSync(new URL('../../../.github/workflows/ci.yml', import.meta.url), 'utf8'),
);
for (const job of ['cross-platform', 'macos-msrv']) {
  const steps = ci.jobs[job].steps;
  const helperBuild = steps.findIndex((step) => step.run === 'npm run build:native-capsule:dev');
  const hostBuild = steps.findIndex((step) =>
    /cargo[^\n]*--manifest-path src-tauri\/Cargo.toml/.test(step.run ?? ''),
  );
  assert.ok(
    helperBuild >= 0 && helperBuild < hostBuild,
    job + ' must build the real externalBin before compiling Tauri',
  );
  assert.equal(
    steps[helperBuild].if,
    undefined,
    job + ' must prepare the helper on every desktop platform',
  );
}
console.log(
  'native capsule build: output directories, target selection, platform hooks and CI prerequisites passed',
);
