import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { load } from 'js-yaml';

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
const activeExtensions =
  /(?:\.(?:[cm]?[jt]sx?|sh|zsh|ps1|cmd|py|rs|toml|json|json5|ya?ml|md|txt|patch|sln|vcxproj|props|targets|nsh|wxs)|(?:^|\/)CMakeLists\.txt)$/;
for (const path of files) {
  // AOCI indexes are maintained in a separate task and may describe old trees.
  if (!activeExtensions.test(path) || path.startsWith('.aoci/') || /^aoci[^/]*\.txt$/.test(path))
    continue;
  // This tool reads immutable tar trees from released tags, where these paths
  // really existed; its current-client side already uses app/Cargo.toml.
  if (path === 'scripts/test-sync-v1-released.py') {
    const source = readFileSync(resolve(repo, path), 'utf8');
    for (const line of source.split('\n')) {
      if (oldPlatform.test(line)) assert.ok(line.includes('extract(ROOT, RELEASES[release]'));
    }
    continue;
  }
  // This patch addresses the iOS prepared mirror's internal symlink, not the
  // repository layout. prepare-core.py maps that symlink to the Linux root.
  if (path === 'openless-all/ios/CoreHost/Patches/ios-input-contracts.patch') continue;
  const source = readFileSync(resolve(repo, path), 'utf8');
  assert.ok(!oldPlatform.test(source), `obsolete platform path: ${path}`);
  assert.ok(!oldBundle.test(source), `obsolete bundle path: ${path}`);
  for (const name of scriptNames) {
    const oldScript = ['openless-all', 'app', 'scripts', name].join('/');
    assert.ok(!source.includes(oldScript), `obsolete script path in ${path}: ${name}`);
  }
}

// These are Android JVM class names, never filesystem paths. Keep the vendored
// JNI contract exact even though "android/" is now outside the app directory.
const androidMedia = read('app/src-tauri/vendor/cpal/src/host/oboe/android_media.rs');
for (const name of ['AudioTrack', 'AudioRecord']) {
  assert.ok(androidMedia.includes(`"android/media/${name}"`));
}
assert.ok(!androidMedia.includes('"../android/media/'));

const movedNames = new Set(scriptNames);
const tokens = (text) =>
  (text.match(/"[^"]*"|'[^']*'|[^\s;|&)<>`]+/g) ?? []).map((value) =>
    value.replace(/^['"]|['"]$/g, ''),
  );
const generatedPath = (path) =>
  // Build outputs and tools explicitly downloaded/fetched by these workflows.
  /^(?:\.\/)?(?:target\/|src-tauri\/(?:target|gen)\/|node_modules\/|release-tooling\/|release-files\/)/.test(
    path,
  );

function validateWorkflow(source, name = 'fixture.yml') {
  const workflow = load(source);
  for (const [jobName, job] of Object.entries(workflow.jobs ?? {})) {
    for (const step of job.steps ?? []) {
      if (!step.run) continue;
      const cwd =
        step['working-directory'] ??
        job.defaults?.run?.['working-directory'] ??
        workflow.defaults?.run?.['working-directory'] ??
        '.';
      if (cwd.includes('$')) continue;
      const root = resolve(repo, cwd);
      const context = `${name}/${jobName}/${step.name ?? step.id ?? 'run'} (cwd=${cwd})`;
      const check = (operand) => {
        const path = operand.replaceAll('\\', '/').replace(/^\.\//, '');
        if (
          !path ||
          path === '-' ||
          path.includes('$') ||
          path.includes('*') ||
          path.includes('{') ||
          path.startsWith('/') ||
          generatedPath(path)
        )
          return;
        if (root === app) {
          assert.ok(
            !/^(?:linux-egui|windows-ime|harmony|android)\//.test(path) &&
              !(path.startsWith('scripts/') && movedNames.has(path.slice(8))),
            `obsolete app-relative input ${operand}: ${context}`,
          );
        }
        assert.ok(existsSync(resolve(root, path)), `missing workflow input ${operand}: ${context}`);
      };
      // Historical fallback exceptions are intentionally narrow: a new source
      // path must be tested first, the old file must have its own existence
      // guard, and a missing-both branch must fail explicitly.
      const guardedOldPaths = new Set();
      for (const match of step.run.matchAll(
        /if \[ -f (\.\.\/(?:harmony|macos|linux)\/[^\s]+) \]; then\n\s+(\w+)=\1\n\s*elif \[ -f ((?:scripts|linux-egui)\/[^\s]+) \]; then\n\s+\2=\3\n\s*else\n\s+echo "::error::Missing source input:[^\n]+\n\s+exit 1\n\s*fi/g,
      )) {
        assert.ok(name.startsWith('release-'), `historical fallback outside a release: ${context}`);
        check(match[1]);
        guardedOldPaths.add(match[3]);
      }
      for (const match of step.run.matchAll(
        /if \(Test-Path -LiteralPath '(\.\.\/windows\/scripts\/[^']+)' -PathType Leaf\) \{\n\s+\$(\w+) = '\1'\n\s*\} elseif \(Test-Path -LiteralPath '(scripts\/[^']+)' -PathType Leaf\) \{\n\s+\$\2 = '\3'\n\s*\} else \{\n\s+throw "Missing source input:[^\n]+\n\s*\}/g,
      )) {
        assert.ok(
          name === 'release-tauri.yml',
          `historical fallback outside desktop release: ${context}`,
        );
        check(match[1]);
        guardedOldPaths.add(match[3]);
      }
      for (const line of step.run.replace(/\\\r?\n/g, ' ').split('\n')) {
        const text = line.trim();
        if (!text || text.startsWith('#')) continue;
        // Check bare legacy inputs even in assignments or pathspecs. Only the
        // exact guarded historical branches above may name an old source input.
        if (root === app && !/^(?:echo|Write-Host|throw)\b/.test(text)) {
          for (const token of tokens(text)) {
            const path = token.replace(/^[^=]+=/, '').replace(/^\.\//, '');
            if (
              /^(?:linux-egui|windows-ime|harmony|android)\//.test(path) ||
              (path.startsWith('scripts/') && movedNames.has(path.slice(8)))
            ) {
              assert.ok(
                guardedOldPaths.has(path),
                `obsolete app-relative input ${path}: ${context}`,
              );
            }
          }
        }
        for (const match of text.matchAll(
          /(?:^|\$\(|[;&|]\s*)(node|bash|python3?|swift|cat|git diff)\s+([^;|)]+)/g,
        )) {
          const [, command, argumentsText] = match;
          if (argumentsText.includes('<<')) continue; // heredoc delimiters are not file inputs
          const args = tokens(argumentsText);
          if (command === 'git diff' || command === 'cat') {
            for (const arg of args) {
              if (arg.startsWith('-') || arg === '\\' || arg === 'dev/null') continue;
              // Git revision names are not source operands.
              if (command === 'git diff' && !arg.includes('/')) continue;
              check(arg);
            }
          } else {
            // -e/-p/-c and stdin are code, while -B/-I/--import are flags.
            if (
              args.some((arg) => ['-e', '-p', '-c', '--eval', '--print'].includes(arg)) ||
              argumentsText.trim().startsWith('<<')
            )
              continue;
            let index = 0;
            while (args[index]?.startsWith('-')) {
              const flag = args[index++];
              if (['--import', '--loader', '--require', '-r'].includes(flag)) index++;
            }
            const input = args[index];
            if (!input || ['-', 'test', 'build', 'package', 'run'].includes(input)) continue;
            check(input);
          }
        }
        const powershell = text.match(
          /^(?:&\s+)?['"]?((?:\.\/|\.\.\/|openless-all\/)[^'"\s]+\.ps1)['"]?(?:\s|$)/,
        );
        if (powershell) check(powershell[1]);
      }
    }
  }
}

const workflowDir = resolve(repo, '.github/workflows');
for (const name of readdirSync(workflowDir).filter((path) => path.endsWith('.yml'))) {
  validateWorkflow(readFileSync(resolve(workflowDir, name), 'utf8'), name);
}
const ciSource = readFileSync(resolve(workflowDir, 'ci.yml'), 'utf8');
for (const [good, bad] of [
  ['cat ../linux/egui/package-revision', 'cat linux-egui/package-revision'],
  [
    'node ../harmony/scripts/audit-harmony-ipc.mjs --check',
    'git diff --exit-code harmony/ipc-audit.json',
  ],
  ['bash ../harmony/scripts/prepare-harmony-sdk.sh', 'bash scripts/prepare-harmony-sdk.sh'],
]) {
  const mutated = ciSource.replace(good, bad);
  assert.notEqual(mutated, ciSource, `mutation must apply: ${bad}`);
  assert.throws(() => validateWorkflow(mutated, 'ci.yml'), /obsolete app-relative input/);
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
