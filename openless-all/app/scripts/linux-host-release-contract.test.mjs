// Linux 交付链的契约：CI、打包脚本、发版入口三者必须指向同一套构建方式。
//
// 结构约定（与其他平台一致）：Linux 不占用独立工作流文件。
//   * CI：`.github/workflows/ci.yml` 里的 `linux-check`（测试）与 `linux-package`（打包）
//   * 打包步骤：`.github/actions/build-linux-packages/action.yml`（CI 与发版共用）
//   * 发版：`.github/workflows/release-tauri.yml` 里的 `linux` 作业
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../../..', import.meta.url));
const load = (path) => readFile(join(root, path), 'utf8');
const exists = (path) => existsSync(join(root, path));

const [ci, tauri, action, pack, verify, gate, manifest, revisionFile, buildScript, host] =
  await Promise.all([
    load('.github/workflows/ci.yml'),
    load('.github/workflows/release-tauri.yml'),
    load('.github/actions/build-linux-packages/action.yml'),
    load('openless-all/app/scripts/package-linux-qt.sh'),
    load('openless-all/app/scripts/verify-linux-host-packages.sh'),
    load('openless-all/app/scripts/check-core-deps.ps1'),
    load('openless-all/app/linux-host/Cargo.toml'),
    load('openless-all/app/linux-host/package-revision'),
    load('openless-all/app/linux-host/build.rs'),
    load('openless-all/app/linux-host/src/update_check.rs'),
  ]);

// ── 1. 一个平台一个文件：Linux 不再有自己的工作流 ──────────────────────────
for (const name of ['check-linux.yml', 'release-linux.yml']) {
  assert.equal(exists(`.github/workflows/${name}`), false, `${name} must not exist`);
}

// ── 2. CI：测试与打包是两个并行作业，都按 changes 的区域门禁执行 ─────────────
assert.deepEqual(
  [...ci.matchAll(/^  (linux-[\w-]+):/gm)].map((match) => match[1]),
  ['linux-check', 'linux-package'],
  'ci.yml must expose exactly the Linux test and package jobs',
);
for (const job of ['linux-check', 'linux-package']) {
  const start = ci.indexOf(`\n  ${job}:`);
  const nextJob = ci.slice(start + 1).search(/\n  [a-z][\w-]*:\n/);
  const body = ci.slice(start, start + 1 + nextJob);
  assert.match(body, /needs: \[changes\]/, `${job} must wait for the area detection`);
  assert.match(body, /needs\.changes\.outputs\.linux != 'false'/, `${job} must be fail-open`);
  assert.match(body, /runs-on: ubuntu-24\.04/, `${job} must pin the packaging baseline`);
}
assert.match(ci, /cargo test --locked -p openless-core/);
assert.match(ci, /cargo test --locked -p openless-linux-host --all-targets/);
for (const crate of ['openless-core', 'openless-linux-host']) {
  assert.ok(ci.includes(`cargo clippy --locked -p ${crate} --all-targets -- -D warnings`), crate);
}
assert.match(
  ci,
  /cargo test --locked --manifest-path src-tauri\/backend-tests\/Cargo\.toml --test remote_tls/,
);
// `cargo test --all-targets` 已经编译了全部 target，再加一次 `cargo check` 是白编译。
assert.doesNotMatch(ci, /cargo check --locked -p openless-linux-host/);
assert.match(ci, /check-core-deps\.ps1 openless-linux-host/);
assert.match(ci, /ctest --test-dir build --output-on-failure/);

// ── 3. 打包：只有一份定义，CI 与发版共用 ────────────────────────────────────
assert.match(ci, /uses: \.\/\.github\/actions\/build-linux-packages/, 'CI must use the shared action');
assert.match(tauri, /uses: \.\/\.github\/actions\/build-linux-packages/, 'release must use it too');
assert.match(action, /cargo build --locked --release -p openless-linux-host/);
assert.match(action, /bash scripts\/package-linux-qt\.sh/);
assert.match(action, /sha256sum \.\/\*\.deb \.\/\*\.rpm > SHA256SUMS/);
assert.match(action, /bash scripts\/verify-linux-host-packages\.sh/);
assert.match(action, /OpenLess_linux_x86_64\.deb/);
assert.match(action, /OpenLess_linux_x86_64\.rpm/);
assert.match(action, /openless-linux-host-direct-download/);
assert.match(action, /REVISION_FILE/);
assert.match(action, /version=\$\{VERSION%%\+\*\}-\$REVISION/);
assert.match(ci, /openless-linux-host-direct-download/, 'CI publishes the direct-download aliases');

// ── 4. 发版：同一个工作流里的 linux 作业，且必须有真机验收记录 ──────────────
assert.match(tauri, /^  linux:$/m, 'release-tauri.yml must contain the Linux job');
assert.match(tauri, /vars\.LINUX_ACCEPTED_RELEASE_TAG == github\.ref_name/);
assert.match(tauri, /gh release upload "\$RELEASE_TAG"/);
assert.match(tauri, /OpenLess-Linux-\*\.deb/);
assert.ok(!tauri.includes('build-linux-host:'), 'Linux does not build Tauri');

// ── 5. 版本与依赖边界 ──────────────────────────────────────────────────────
assert.match(gate, /cargo tree --locked/);
assert.match(gate, /tauri\|wry\|webkit2gtk/);
assert.doesNotMatch(manifest, /legacy-preferences-write/);

const revision = revisionFile.trim();
assert.match(revision, /^[1-9][0-9]*$/, 'Linux package revision must be a positive integer');
assert.ok(Number(revision) > 79, 'new Linux packages must upgrade the last -79 candidate');
assert.match(action, /linux-host\/package-revision/);
assert.match(buildScript, /rerun-if-changed=package-revision/);
assert.match(buildScript, /rerun-if-env-changed=OPENLESS_LINUX_VERSION/);
assert.match(buildScript, /OPENLESS_APP_VERSION=\{expected\}/);
assert.match(host, /env!\("OPENLESS_APP_VERSION"\)/);
assert.match(verify, /DEB_VERSION=\$\(dpkg-deb --field/);
assert.match(verify, /OpenLess \$DEB_VERSION/);
assert.match(pack, /linux-host\/package-revision/);
assert.match(pack, /if \[ "\$VERSION" != "\$EXPECTED_VERSION" \]/);

console.log('linux-host-release contract test passed');
