import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const app = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repo = resolve(app, '../..');
const all = resolve(app, '..');
const read = (path) => readFileSync(resolve(all, path), 'utf8');
const required = [
  'android/kotlin/test',
  'android/kotlin/androidTest',
  'android/frontend/components/AndroidPermissionsPanel.tsx',
  'android/manifests/AndroidManifest.v1.snippet.xml',
  'android/aidl',
  'harmony/entry/src/main/cpp/CMakeLists.txt',
  'harmony/rust/Cargo.toml',
  'linux/egui/Cargo.toml',
  'linux/egui/package-revision',
  'linux/fcitx5-plugin/CMakeLists.txt',
  'windows/ime/OpenLessIme.sln',
  'windows/nsis/openless-ime-hooks.nsh',
  'windows/wix/openless-ime.wxs',
  'macos/Info.plist',
  'macos/Entitlements.plist',
  'macos/dmg/layout.json',
  'macos/dmg/installer-background.tiff',
  'macos/dmg/installer-background@2x.png',
  'app/mobile-ime/CMakeLists.txt',
  'app/scripts/build-mobile-rime.py',
  'app/src-tauri/icons/android',
];
for (const path of required) assert.ok(existsSync(resolve(all, path)), path);

const config = JSON.parse(read('app/src-tauri/tauri.conf.json'));
const macConfig = JSON.parse(read('app/src-tauri/tauri.macos.conf.json'));
const layout = JSON.parse(read('macos/dmg/layout.json'));
for (const path of [
  config.bundle.macOS.infoPlist,
  config.bundle.macOS.entitlements,
  config.bundle.windows.nsis.installerHooks,
  ...config.bundle.windows.wix.fragmentPaths,
  macConfig.bundle.macOS.dmg.background,
  layout.retinaBackground,
]) {
  assert.ok(existsSync(resolve(app, 'src-tauri', path)), `Tauri asset: ${path}`);
}
for (const member of ['linux/egui', 'harmony/rust']) {
  assert.match(read(`${member}/Cargo.toml`), /workspace = "\.\.\/\.\.\/app"/);
  assert.match(read(`${member}/Cargo.toml`), /path = "\.\.\/\.\.\/app\/crates\/openless-core"/);
  assert.ok(read('app/Cargo.toml').includes(`"../${member}"`));
}

const files = execFileSync('git', ['ls-files', '-z'], { cwd: repo, encoding: 'utf8' })
  .split('\0')
  .filter(Boolean);
const movedScripts = files.filter((path) =>
  /^openless-all\/(?:android|harmony|linux|windows|macos)\/scripts\//.test(path),
);
assert.ok(movedScripts.length >= 80, 'platform operational scripts must remain tracked');
for (const path of movedScripts) {
  assert.ok(existsSync(resolve(repo, path)), path);
  assert.ok(
    !existsSync(resolve(app, 'scripts', path.split('/scripts/')[1])),
    `duplicate app script: ${path}`,
  );
}
assert.ok(
  files.filter((path) => /^openless-all\/app\/scripts\/[^/]+\.test\.mjs$/.test(path)).length >= 109,
  'all original 108 script contracts and this layout contract remain discoverable',
);

const oldPlatform = new RegExp(
  ['openless-all', 'app', '(?:android|harmony|linux-egui|windows-ime)(?:/|\\b)'].join('/'),
);
const oldBundle = new RegExp(
  ['src-tauri', '(?:dmg|nsis|wix)(?:/|\\b)|src-tauri/(?:Info|Entitlements)\\.plist'].join('/'),
);
const scriptNames = movedScripts.map((path) => path.split('/scripts/')[1]);
const activeExtensions = /\.(?:[cm]?[jt]sx?|sh|ps1|cmd|py|rs|toml|json|json5|ya?ml|md|nsh|wxs)$/;
for (const path of files) {
  if (!activeExtensions.test(path) || path.startsWith('.aoci/')) continue;
  // This tool reads immutable tar trees from released tags, where these paths
  // really existed; its current-client side already uses app/Cargo.toml.
  if (path === 'scripts/test-sync-v1-released.py') {
    const source = readFileSync(resolve(repo, path), 'utf8');
    for (const line of source.split('\n')) {
      if (oldPlatform.test(line)) assert.ok(line.includes('extract(ROOT, RELEASES[release]'));
    }
    continue;
  }
  const source = readFileSync(resolve(repo, path), 'utf8');
  assert.ok(!oldPlatform.test(source), `obsolete platform path: ${path}`);
  assert.ok(!oldBundle.test(source), `obsolete bundle path: ${path}`);
  for (const name of scriptNames) {
    const oldScript = ['openless-all', 'app', 'scripts', name].join('/');
    assert.ok(!source.includes(oldScript), `obsolete script path in ${path}: ${name}`);
  }
}

const mirror = read('windows/scripts/windows-build-gnu.ps1');
assert.match(mirror, /git -c core\.quotePath=false ls-files --cached --others --exclude-standard/);
assert.match(mirror, /Join-Path \$MirrorRoot "app"/);
assert.match(mirror, /Copy-Item -LiteralPath \(Join-Path \$allRoot \$relative\)/);
const frontend = read('harmony/scripts/build-harmony-frontend.sh');
assert.match(frontend, /"\$APP_ROOT\/mobile-ime"/);
assert.match(frontend, /shutil\.copytree\(mobile_ime, target \/ 'mobile-ime'/);
assert.match(read('windows/wix/openless-ime.wxs'), /Source="\$\(env\.OPENLESS_IME_DLL_X64\)"/);
assert.match(read('windows/wix/openless-ime.wxs'), /Source="\$\(env\.OPENLESS_IME_DLL_X86\)"/);
console.log('Platform directory layout, source paths, and bundle assets passed');
