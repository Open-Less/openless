import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../../..', import.meta.url));
const load = (path) => readFile(join(root, path), 'utf8');
const [release, ci, check, pack, verify, gate, manifest, tauri, revisionFile] = await Promise.all([
  load('.github/workflows/release-linux-egui.yml'),
  load('.github/workflows/ci.yml'),
  load('.github/workflows/check-linux-egui.yml'),
  load('openless-all/app/scripts/package-linux-egui.sh'),
  load('openless-all/app/scripts/verify-linux-egui-packages.sh'),
  load('openless-all/app/scripts/check-core-deps.ps1'),
  load('openless-all/app/linux-egui/Cargo.toml'),
  load('.github/workflows/release-tauri.yml'),
  load('openless-all/app/linux-egui/package-revision'),
]);

// CI must execute the exact test/build/package/checksum path used for tags.
assert.match(ci, /linux-egui-package:[\s\S]*?uses: \.\/\.github\/workflows\/check-linux-egui\.yml/);
assert.match(ci, /linux-egui-package:[\s\S]*?permissions:\s*contents: read/);
assert.deepEqual(
  [...ci.matchAll(/^  (linux-[\w-]+):/gm)].map((match) => match[1]),
  ['linux-egui-package'],
  'PR CI must expose one Linux job, including Core and remote TLS checks',
);
assert.match(release, /uses: \.\/\.github\/workflows\/check-linux-egui\.yml/);
assert.match(
  check,
  /^on:\n  workflow_call:/m,
  'the reusable check must not run as a duplicate standalone workflow',
);
assert.match(check, /contents: read/);
assert.match(check, /runs-on: ubuntu-24\.04/);
assert.match(check, /cargo test --locked -p openless-core/);
assert.match(
  check,
  /cargo test --locked --manifest-path src-tauri\/backend-tests\/Cargo\.toml --test remote_tls/,
);
assert.match(check, /cargo test --locked -p openless-linux-egui --all-targets/);
assert.match(check, /cargo check --locked -p openless-linux-egui --all-targets/);
assert.match(check, /check-core-deps\.ps1 openless-linux-egui/);
assert.match(check, /ctest --test-dir build --output-on-failure/);
assert.match(check, /cargo build --locked --release -p openless-linux-egui/);
assert.match(check, /bash scripts\/package-linux-egui\.sh/);
assert.match(check, /sha256sum \.\/\*\.deb \.\/\*\.rpm > SHA256SUMS/);
assert.match(check, /bash scripts\/verify-linux-egui-packages\.sh/);
assert.match(gate, /cargo tree --locked/);
assert.match(gate, /tauri\|wry\|webkit2gtk/);
assert.doesNotMatch(ci + check, /libgtk-3|webkit2gtk|libwebkit/);

// An admin's existing Tauri tag launches an independent Linux build. No PR or
// manual dispatch can attach assets; Beta remains a shared draft for review.
assert.match(release, /tags:\s*\n\s*- 'v\*-tauri'/);
assert.match(
  release,
  /if: github\.event_name == 'push' && startsWith\(github\.ref, 'refs\/tags\/v'\) && endsWith\(github\.ref, '-tauri'\)/,
);
assert.match(release, /needs: build-linux-egui/);
assert.match(release, /contents: write/);
assert.match(release, /softprops\/action-gh-release@v2/);
assert.match(release, /draft:.*Beta\./);
assert.match(release, /prerelease:.*Beta\./);
assert.match(release, /tag_name: \$\{\{ github\.ref_name \}\}/);
assert.match(check, /does not match package\.json version/);
const revision = revisionFile.trim();
assert.match(revision, /^[1-9][0-9]*$/, 'Linux package revision must be a positive integer');
assert.ok(Number(revision) > 79, 'new Linux packages must upgrade the last -79 candidate');
assert.match(check, /REVISION=\$\(< linux-egui\/package-revision\)/);
assert.match(check, /version=\$\{VERSION%%\+\*\}-\$REVISION/);
assert.match(pack, /linux-egui\/package-revision/);
assert.match(pack, /if \[ "\$VERSION" != "\$EXPECTED_VERSION" \]/);
assert.ok(!tauri.includes('build-linux-egui:'), 'Linux does not build Tauri');
assert.ok(!release.includes('release_tag:'), 'the manual build cannot target a Release');

// Keep deb/rpm only, including dynamically-loaded desktop libraries and the
// fcitx5 addon. No AppImage, minisign, embedded downloader or Qwen ASR runtime.
assert.match(pack, /dpkg-deb --build/);
assert.match(pack, /rpmbuild --define/);
assert.match(pack, /DEB_LIB_ARCH=x86_64-linux-gnu/);
assert.match(pack, /usr\/lib\/\$DEB_LIB_ARCH\/fcitx5\/libopenless\.so/);
assert.match(pack, /\/usr\/lib64\/fcitx5\/libopenless\.so/);
assert.doesNotMatch(pack, /appimagetool|AppImage|qwen-asr|qwen_asr/i);
assert.doesNotMatch(release + check, /appimagetool|APPIMAGE_|MINISIGN|latest-linux-egui/i);
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
for (const dep of ['fcitx5', 'libpipewire-0.3-0', 'libegl1', 'liblzma5', 'libwayland-egl1']) {
  assert.ok(debDeps.includes(dep), `deb must require ${dep}`);
}
const rpmDeps = pack.match(/^Requires: ([^\n]+)/m)?.[1].split(/,\s*/) ?? [];
for (const dep of ['fcitx5', 'pipewire-libs', 'libglvnd-egl', 'libwayland-egl.so.1()(64bit)']) {
  assert.ok(rpmDeps.includes(dep), `rpm must require ${dep}`);
}
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
assert.doesNotMatch(pack + release, /src-tauri/);
assert.doesNotMatch(
  check,
  /cargo (?:build|check|test) --locked --manifest-path src-tauri\/Cargo\.toml/,
);
console.log('linux-egui-release-contract.test.mjs passed');
