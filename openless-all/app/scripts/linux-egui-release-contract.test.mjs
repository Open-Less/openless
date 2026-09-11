import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = fileURLToPath(new URL('../../..', import.meta.url));
const releaseWorkflow = await readFile(
  join(repoRoot, '.github/workflows/release-linux-egui.yml'),
  'utf8',
);
const ciWorkflow = await readFile(
  join(repoRoot, '.github/workflows/ci.yml'),
  'utf8',
);
const packageScript = await readFile(
  join(repoRoot, 'openless-all/app/scripts/package-linux-egui.sh'),
  'utf8',
);
const dependencyGate = await readFile(
  join(repoRoot, 'openless-all/app/scripts/check-core-deps.ps1'),
  'utf8',
);

// 1. CI must exercise the Tauri-free Linux host: build, test, clippy and a
//    dependency contract on openless-linux-egui with no WebKit/GTK native deps.
assert.ok(ciWorkflow.includes('cargo test --locked -p openless-linux-egui --all-targets'),
  'CI must test the Linux egui host');
assert.ok(ciWorkflow.includes('cargo check --locked -p openless-linux-egui --all-targets'),
  'CI must check the Linux egui host');
assert.ok(ciWorkflow.includes('./scripts/check-core-deps.ps1 openless-linux-egui'),
  'CI must run the cargo-tree dependency gate for the Linux egui host');
assert.ok(!/libgtk-3|webkit2gtk|libwebkit/.test(ciWorkflow),
  'CI apt list must not pull WebKitGTK/GTK native dependencies');

// 2. The Linux dependency gate must forbid Tauri/Wry/WebKit at the cargo-tree
//    level while still allowing egui/eframe for the host itself.
assert.ok(dependencyGate.includes('cargo tree --locked'),
  'dependency gate must run cargo tree');
assert.ok(dependencyGate.includes('openless-linux-egui'),
  'dependency gate must accept the Linux egui host package name');
assert.ok(/tauri\|wry\|webkit2gtk/.test(dependencyGate),
  'dependency gate must forbid tauri/wry/webkit2gtk');

// 3. Release must build x86_64 deb and rpm only; AppImage is intentionally
// retired from the Linux distribution channel.
for (const [format, tool] of [['deb', 'fpm -s dir -t deb'], ['rpm', 'fpm -s dir -t rpm']]) {
  assert.ok(packageScript.includes(tool), `package script must build ${format} via ${tool}`);
}
assert.ok(releaseWorkflow.includes("test \"$(find \"$OUTPUT\" -maxdepth 1 -name '*.deb' | wc -l)\" -eq 1"),
  'release must gate exactly one deb');
assert.ok(releaseWorkflow.includes("test \"$(find \"$OUTPUT\" -maxdepth 1 -name '*.rpm' | wc -l)\" -eq 1"),
  'release must gate exactly one rpm');
assert.ok(!/appimagetool|AppImage|APPIMAGE/i.test(packageScript),
  'Linux package script must not build or stage AppImage');

// 4. deb/rpm must carry the host and fcitx5 addon. Qwen ASR is
//    deliberately excluded from Linux packages and must never enter this flow.
assert.ok(releaseWorkflow.includes("! ldd target/release/openless-linux-egui | grep -q 'not found'"),
  'release must run an ldd gate on the host binary');
assert.ok(/! ldd .*grep -Eqi 'webkit\|wry\|tauri'/.test(releaseWorkflow),
  'release must assert the host binary does not link WebKit/Wry/Tauri');
assert.ok(releaseWorkflow.includes("dpkg-deb -c \"$OUTPUT\"/*.deb | grep -q 'usr/bin/openless'"),
  'deb must ship the host binary');
assert.ok(releaseWorkflow.includes("dpkg-deb -c \"$OUTPUT\"/*.deb | grep -q 'fcitx5/libopenless.so'"),
  'deb must ship the fcitx5 addon');
assert.ok(releaseWorkflow.includes("rpm -qlp \"$OUTPUT\"/*.rpm | grep -q '/usr/bin/openless'"),
  'rpm must ship the host binary');
assert.ok(releaseWorkflow.includes("rpm -qlp \"$OUTPUT\"/*.rpm | grep -q '/usr/lib64/fcitx5/libopenless.so'"),
  'rpm must ship the fcitx5 addon');
assert.ok(!/qwen-asr|qwen_asr/i.test(releaseWorkflow),
  'Linux release must neither fetch nor compile nor package Qwen ASR');
assert.ok(!/qwen-asr|qwen_asr/i.test(packageScript),
  'Linux packaging must not stage Qwen ASR');

// The packaging script must carry the fcitx plugin into both package formats.
assert.ok(packageScript.includes('x86_64-linux-gnu/fcitx5/libopenless.so'), 'deb fcitx addon path');
assert.ok(packageScript.includes('/usr/lib64/fcitx5/libopenless.so'), 'rpm fcitx addon path');
// 5. ldd + cargo-tree verification gates are required for release and CI.

// 6. Release must emit and verify checksums for both package artifacts.
assert.ok(releaseWorkflow.includes('sha256sum'), 'release must compute sha256 checksums');
assert.ok(releaseWorkflow.includes('> SHA256SUMS'), 'release must emit a SHA256SUMS artifact');
assert.ok(releaseWorkflow.includes('sha256sum -c SHA256SUMS'), 'release must verify SHA256SUMS');
assert.ok(releaseWorkflow.includes('sha256sum ./*.deb ./*.rpm'),
  'SHA256SUMS must cover deb and rpm only');
assert.ok(!/appimagetool|APPIMAGE_|LINUX_EGUI_MINISIGN|latest-linux-egui/i.test(releaseWorkflow),
  'Linux release must not retain an AppImage updater or signing path');

// 8. Legacy "-tauri" release tags are accepted only for compatibility: the
//    suffix is stripped when present and the flow still resolves its own version
//    from cargo metadata when no release tag is supplied, so it never depends on
//    the Tauri release pipeline or its tag scheme.
assert.ok(releaseWorkflow.includes('VERSION=${RELEASE_TAG#v}'), 'version must strip a leading v');
assert.ok(releaseWorkflow.includes('VERSION=${VERSION%-tauri}'), 'legacy -tauri tag suffix must be tolerated');
assert.ok(releaseWorkflow.includes('RELEASE_TAG:-}'), 'flow must run without a release tag');
assert.ok(/cargo metadata[\s\S]*select\(\.name == "openless-linux-egui"\)/.test(releaseWorkflow),
  'flow must resolve its version from cargo metadata when no tag is provided');
assert.ok(!releaseWorkflow.includes('-tauri required') && !/case "\$RELEASE_TAG"[\s\S]*\*-tauri/.test(releaseWorkflow),
  'flow must not mandate a -tauri release tag');

// 9. The release flow never touches the Tauri host or its src-tauri tree.
assert.ok(!/src-tauri/.test(packageScript), 'package script must not reference src-tauri');
assert.ok(!/src-tauri/.test(releaseWorkflow), 'release workflow must not reference src-tauri');
assert.ok(!/\btauri\b|\bwry\b/.test(packageScript), 'package script must not invoke Tauri tooling');

console.log('linux-egui-release-contract.test.mjs passed');
