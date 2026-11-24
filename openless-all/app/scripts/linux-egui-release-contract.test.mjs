// Linux automation is paused, but the local packaging/source contracts remain.
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../../..', import.meta.url));
const load = (path) => readFile(join(root, path), 'utf8');
const [ci, tauri, pack, verify, gate, manifest, revisionFile, buildScript, host] =
  await Promise.all([
    load('.github/workflows/ci.yml'),
    load('.github/workflows/release-tauri.yml'),
    load('openless-all/app/scripts/package-linux-egui.sh'),
    load('openless-all/app/scripts/verify-linux-egui-packages.sh'),
    load('openless-all/app/scripts/check-core-deps.ps1'),
    load('openless-all/app/linux-egui/Cargo.toml'),
    load('openless-all/app/linux-egui/package-revision'),
    load('openless-all/app/linux-egui/build.rs'),
    load('openless-all/app/linux-egui/src/main.rs'),
  ]);
for (const name of ['check-linux-egui.yml', 'release-linux-egui.yml']) {
  assert.equal(
    existsSync(join(root, '.github/workflows', name)),
    false,
    `${name}: Linux automation is paused`,
  );
}
assert.doesNotMatch(ci, /linux-egui-package:|check-linux-egui\.yml/);
assert.doesNotMatch(tauri, /release-linux-egui\.yml|LINUX_ACCEPTED_RELEASE_TAG|build-linux-egui:/);
assert.doesNotMatch(ci, /libgtk-3|webkit2gtk|libwebkit/);
assert.match(gate, /cargo tree --locked/);
assert.match(gate, /tauri\|wry\|webkit2gtk/);

const revision = revisionFile.trim();
assert.match(revision, /^[1-9][0-9]*$/, 'positive Linux package revision');
assert.ok(Number(revision) > 79, 'local packages must upgrade the last -79 candidate');
assert.match(buildScript, /rerun-if-changed=package-revision/);
assert.match(buildScript, /rerun-if-env-changed=OPENLESS_LINUX_VERSION/);
assert.match(buildScript, /OPENLESS_APP_VERSION=\{expected\}/);
assert.match(host, /env!\("OPENLESS_APP_VERSION"\)/);
assert.match(verify, /DEB_VERSION=\$\(dpkg-deb --field/);
assert.match(verify, /OpenLess \$DEB_VERSION/);
assert.match(pack, /linux-egui\/package-revision/);
assert.match(pack, /if \[ "\$VERSION" != "\$EXPECTED_VERSION" \]/);
assert.match(pack, /dpkg-deb --build/);
assert.match(pack, /rpmbuild --define/);
assert.match(pack, /x86_64-linux-gnu\/fcitx5\/libopenless\.so/);
assert.match(pack, /\/usr\/lib64\/fcitx5\/libopenless\.so/);
assert.match(pack, /require_file "\$PLUGIN_ROOT\/libopenless\.so"/);
assert.doesNotMatch(pack, /appimagetool|AppImage|qwen-asr|qwen_asr|src-tauri/i);
assert.match(verify, /test "\$\{#debs\[@\]\}" -eq 1/);
assert.match(verify, /test "\$\{#rpms\[@\]\}" -eq 1/);
assert.match(verify, /test "\$\{#appimages\[@\]\}" -eq 0/);
assert.match(verify, /sha256sum --check --strict SHA256SUMS/);
assert.match(verify, /desktop-file-validate/);
assert.match(verify, /check_elf "\$WORK\/deb\/usr\/bin\/openless"/);
assert.match(verify, /fcitx5\/libopenless\.so/);
assert.ok(!existsSync(join(root, 'openless-all/app/linux-egui/src/updater.rs')));
assert.doesNotMatch(manifest, /minisign-verify/);
const debDeps = pack.match(/^Depends: ([^\n]+)/m)?.[1].split(/,\s*/) ?? [];
for (const dep of ['fcitx5', 'libpipewire-0.3-0', 'libvulkan1', 'liblzma5', 'libwayland-client0']) {
  assert.ok(debDeps.includes(dep), `deb must require ${dep}`);
}
for (const dep of ['libegl1', 'libgl1', 'libwayland-egl1']) {
  assert.ok(!debDeps.includes(dep), `deb must not require ${dep}`);
}
const rpmDeps = pack.match(/^Requires: ([^\n]+)/m)?.[1].split(/,\s*/) ?? [];
for (const dep of ['fcitx5', 'pipewire-libs', 'vulkan-loader', 'libwayland-client']) {
  assert.ok(rpmDeps.includes(dep), `rpm must require ${dep}`);
}
for (const dep of ['libglvnd-egl', 'libwayland-egl.so.1()(64bit)']) {
  assert.ok(!rpmDeps.includes(dep), `rpm must not require ${dep}`);
}
assert.match(pack, /^Recommends: mesa-vulkan-drivers$/m);
for (const [file, size] of [
  ['32x32.png', '32x32'],
  ['64x64.png', '64x64'],
  ['128x128.png', '128x128'],
  ['128x128@2x.png', '256x256'],
  ['icon.png', '512x512'],
]) {
  const [copy, shared] = await Promise.all([
    readFile(join(root, 'openless-all/app/linux-egui/packaging/icons', file)),
    readFile(join(root, 'openless-all/app/src-tauri/icons', file)),
  ]);
  assert.deepEqual(copy, shared, `${file} must match the shared icon`);
  assert.ok(pack.includes(`${size}:${file}`));
}
console.log('linux-egui-release-contract: automation paused, local packaging contracts passed');
