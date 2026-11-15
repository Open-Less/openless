// Linux automation is paused, but the local packaging/source contracts remain.
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../../..', import.meta.url));
const load = (path) => readFile(join(root, path), 'utf8');
const [
  release, // .github/workflows/release-linux.yml
  ci, // .github/workflows/ci.yml
  check, // .github/workflows/check-linux.yml
  tauri, // .github/workflows/release-tauri.yml
  pack, // scripts/package-linux-qt.sh
  verify, // scripts/verify-linux-host-packages.sh
  gate, // scripts/check-core-deps.ps1
  manifest, // linux-host/Cargo.toml
  revisionFile, // linux-host/package-revision
  buildScript, // linux-host/build.rs
  host, // linux-host/src/update_check.rs
] = await Promise.all([
  load('.github/workflows/release-linux.yml'),
  load('.github/workflows/ci.yml'),
  load('.github/workflows/check-linux.yml'),
  load('.github/workflows/release-tauri.yml'),
  load('openless-all/app/scripts/package-linux-qt.sh'),
  load('openless-all/app/scripts/verify-linux-host-packages.sh'),
  load('openless-all/app/scripts/check-core-deps.ps1'),
  load('openless-all/app/linux-host/Cargo.toml'),
  load('openless-all/app/linux-host/package-revision'),
  load('openless-all/app/linux-host/build.rs'),
  load('openless-all/app/linux-host/src/update_check.rs'),
]);

// CI must execute the exact test/build/package/checksum path used for tags.
assert.match(ci, /linux-host-package:[\s\S]*?uses: \.\/\.github\/workflows\/check-linux.yml/);
assert.match(ci, /linux-host-package:[\s\S]*?permissions:\s*contents: read/);
// The release entry point needs write access to attach assets; the CI caller is
// read-only and must not be able to publish.
assert.match(release, /permissions:\n  contents: write/);
assert.deepEqual(
  [...ci.matchAll(/^  (linux-[\w-]+):/gm)].map((match) => match[1]),
  ['linux-host-package'],
  'PR CI must expose one Linux job, including Core and remote TLS checks',
);
assert.match(release, /uses: \.\/\.github\/workflows\/check-linux.yml/);
assert.match(
  check,
  /^on:\n  workflow_call:/m,
  'the reusable check must not run as a duplicate standalone workflow',
);
assert.match(check, /contents: write/);
assert.match(check, /runs-on: ubuntu-24\.04/);
assert.match(check, /cargo test --locked -p openless-core/);
for (const crate of ['openless-core', 'openless-linux-host']) {
  assert.ok(check.includes(`cargo clippy --locked -p ${crate} --all-targets -- -D warnings`));
}
assert.match(
  check,
  /cargo test --locked --manifest-path src-tauri\/backend-tests\/Cargo\.toml --test remote_tls/,
);
assert.match(check, /cargo test --locked -p openless-linux-host --all-targets/);
// `cargo test --all-targets` already compiles every target, so a following
// `cargo check --all-targets` was a second full compile for no extra coverage.
assert.doesNotMatch(check, /cargo check --locked -p openless-linux-host/);
assert.match(check, /check-core-deps\.ps1 openless-linux-host/);
assert.match(check, /ctest --test-dir build --output-on-failure/);
assert.match(check, /cargo build --locked --release -p openless-linux-host/);
assert.match(check, /bash scripts\/package-linux-qt\.sh/);
assert.match(check, /sha256sum \.\/\*\.deb \.\/\*\.rpm > SHA256SUMS/);
assert.match(check, /bash scripts\/verify-linux-host-packages\.sh/);
assert.match(check, /OpenLess_linux_x86_64\.deb/);
assert.match(check, /OpenLess_linux_x86_64\.rpm/);
assert.match(check, /openless-linux-host-direct-download/);
assert.match(gate, /cargo tree --locked/);
assert.match(gate, /tauri\|wry\|webkit2gtk/);

const revision = revisionFile.trim();
assert.match(revision, /^[1-9][0-9]*$/, 'Linux package revision must be a positive integer');
assert.ok(Number(revision) > 79, 'new Linux packages must upgrade the last -79 candidate');
assert.match(check, /REVISION=\$\(< linux-host\/package-revision\)/);
assert.match(check, /version=\$\{VERSION%%\+\*\}-\$REVISION/);
assert.match(buildScript, /rerun-if-changed=package-revision/);
assert.match(buildScript, /rerun-if-env-changed=OPENLESS_LINUX_VERSION/);
assert.match(buildScript, /OPENLESS_APP_VERSION=\{expected\}/);
assert.match(host, /env!\("OPENLESS_APP_VERSION"\)/);
assert.match(verify, /DEB_VERSION=\$\(dpkg-deb --field/);
assert.match(verify, /OpenLess \$DEB_VERSION/);
assert.match(pack, /linux-host\/package-revision/);
assert.match(pack, /if \[ "\$VERSION" != "\$EXPECTED_VERSION" \]/);
assert.ok(!tauri.includes('build-linux-host:'), 'Linux does not build Tauri');
assert.match(release, /release_tag: \$\{\{ inputs\.tag \}\}/);
assert.match(release, /checkout_ref: \$\{\{ inputs\.tag \}\}/);

// Automatic dispatch is enabled only by administrator acceptance of the exact tag.
assert.match(
  tauri,
  /^  linux:$/m,
  'release-tauri.yml must call the Linux release workflow',
);
assert.match(
  tauri,
  /uses: \.\/\.github\/workflows\/release-linux\.yml/,
  'the Linux job must reuse release-linux.yml',
);
assert.match(
  tauri,
  /vars\.LINUX_ACCEPTED_RELEASE_TAG == github\.ref_name/,
  'automatic dispatch is enabled only by administrator acceptance of the exact tag',
);
assert.match(
  tauri,
  /uses: \.\/\.github\/workflows\/release-linux\.yml\n    with:\n      tag: \$\{\{ github\.ref_name \}\}\n      publish: true/,
  'the Linux hand-off must publish the packages for the same tag',
);

// Keep deb/rpm only, including dynamically-loaded desktop libraries and the
// fcitx5 addon. No AppImage, minisign, embedded downloader or Qwen ASR runtime.
//
// Rendering is Vulkan-only: the GL/EGL paths were removed (`refactor: remove
// Linux host OpenGL rendering`), so the packages depend on the Vulkan loader
// and nothing dlopens libEGL / libwayland-egl any more. If libegl1 or
// libwayland-egl1 comes back, a GL renderer was reintroduced.
assert.match(pack, /dpkg-deb --build/);
assert.match(pack, /rpmbuild --define/);
assert.match(pack, /x86_64-linux-gnu\/fcitx5\/libopenless\.so/);
assert.match(pack, /\/usr\/lib64\/fcitx5\/libopenless\.so/);
assert.doesNotMatch(pack, /appimagetool|AppImage|qwen-asr|qwen_asr/i);
assert.doesNotMatch(release + check, /appimagetool|APPIMAGE_|MINISIGN|latest-linux-host/i);
assert.match(verify, /test "\$\{#debs\[@\]\}" -eq 1/);
assert.match(verify, /test "\$\{#rpms\[@\]\}" -eq 1/);
assert.match(verify, /test "\$\{#appimages\[@\]\}" -eq 0/);
assert.match(verify, /sha256sum --check --strict SHA256SUMS/);
assert.match(verify, /desktop-file-validate/);
assert.match(verify, /check_elf "\$WORK\/deb\/usr\/lib\/openless\/openless-qt"/);
assert.match(verify, /check_elf "\$WORK\/deb\/usr\/lib\/openless\/openless-linux-web"/);
assert.match(verify, /fcitx5\/libopenless\.so/);
assert.ok(!existsSync(join(root, 'openless-all/app/linux-host/src/updater.rs')));
assert.doesNotMatch(manifest, /minisign-verify/);
const debDeps = pack.match(/^Depends: ([^\n]+)/m)?.[1].split(/,\s*/) ?? [];
for (const dep of ['fcitx5', 'libqt6webenginecore6', 'libqt6webenginewidgets6', 'libqt6widgets6', 'liblayershellqtinterface6']) {
  assert.ok(debDeps.includes(dep), `deb must require ${dep}`);
}
for (const dep of ['libegl1', 'libgl1', 'libwayland-egl1']) {
  assert.ok(!debDeps.includes(dep), `deb must not require ${dep}`);
}
const rpmDeps = pack.match(/^Requires: ([^\n]+)/m)?.[1].split(/,\s*/) ?? [];
for (const dep of ['fcitx5', 'qt6-qtwebengine', 'qt6-qtwayland', 'LayerShellQt']) {
  assert.ok(rpmDeps.includes(dep), `rpm must require ${dep}`);
}
for (const dep of ['libglvnd-egl', 'libwayland-egl.so.1()(64bit)']) {
  assert.ok(!rpmDeps.includes(dep), `rpm must not require ${dep}`);
}
assert.match(pack, /^Recommends: .*mesa-vulkan-drivers/m, 'packages must recommend a Vulkan driver');
for (const [file, size] of [
  ['32x32.png', '32x32'],
  ['64x64.png', '64x64'],
  ['128x128.png', '128x128'],
  ['128x128@2x.png', '256x256'],
  ['icon.png', '512x512'],
]) {
  const [copy, shared] = await Promise.all([
    readFile(join(root, 'openless-all/app/linux-host/packaging/icons', file)),
    readFile(join(root, 'openless-all/app/src-tauri/icons', file)),
  ]);
  assert.deepEqual(copy, shared, `${file} must match the shared icon`);
  assert.ok(pack.includes(`${size}:${file}`));
}
assert.doesNotMatch(pack + release, /src-tauri/);
assert.doesNotMatch(
  check,
  /cargo (?:build|check|test) --locked --manifest-path src-tauri\/Cargo\.toml/,
);

// The package installs the fcitx5 plugin, so the packaging job must build it
// even though `ctest` lives in the parallel test job: dropping it made
// `package-linux-qt.sh` fail on an empty `test -s` with no output at all.
{
  const buildJob = check.match(/  build-linux-host:[\s\S]*$/)?.[0] ?? '';
  assert.match(
    buildJob,
    /cmake --build build --parallel/,
    'build-linux-host must compile the fcitx5 plugin that the package installs',
  );
  assert.match(
    pack,
    /require_file "\$PLUGIN_ROOT\/libopenless\.so"/,
    'package-linux-qt.sh must report a missing plugin artifact instead of a bare test -s',
  );
}

assert.match(
  check,
  /cargo test --locked -p openless-core --lib \\\n\s+api::tests::shared_hotkey_edges_own_hold_auto_and_combo_abort_semantics -- --exact/,
);
assert.match(
  check,
  /grep -Fq 'test api::tests::shared_hotkey_edges_own_hold_auto_and_combo_abort_semantics \.\.\. ok'/,
);
assert.match(check, /needs: \[test-linux-host, build-linux-host\]/);
console.log('linux-host-release-contract.test.mjs passed');
