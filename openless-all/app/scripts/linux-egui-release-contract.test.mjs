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
for (const [format, tool] of [['deb', 'dpkg-deb --build'], ['rpm', 'rpmbuild --define']]) {
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
assert.ok(releaseWorkflow.includes("require('./package.json').version"),
  'flow must resolve its version from the main app package when no tag is provided');
assert.ok(!releaseWorkflow.includes('-tauri required') && !/case "\$RELEASE_TAG"[\s\S]*\*-tauri/.test(releaseWorkflow),
  'flow must not mandate a -tauri release tag');

// 9. The release flow never touches the Tauri host or its src-tauri tree.
assert.ok(!/src-tauri/.test(packageScript), 'package script must not reference src-tauri');
assert.ok(!/src-tauri/.test(releaseWorkflow), 'release workflow must not reference src-tauri');
assert.ok(!/\btauri\b|\bwry\b/.test(packageScript), 'package script must not invoke Tauri tooling');

// 9b. 图标：egui 侧不能引用 src-tauri（上一条），但共享同一套图。打包脚本从 Tauri-free
//     的 `linux-egui/packaging/icons/` 装 5 档 hicolor 尺寸；这些副本必须与共享图标源
//     `src-tauri/icons/`（Tauri 的 macOS/Windows/Android 打包也用那份）逐字节一致，
//     否则两份图会静默漂移。以前只装一档，而且把 512×512 的图放进了 256x256 目录。
const iconSizeLadder = [
  ['32x32.png', '32x32'],
  ['64x64.png', '64x64'],
  ['128x128.png', '128x128'],
  ['128x128@2x.png', '256x256'],
  ['icon.png', '512x512'],
];
for (const [file, hicolor] of iconSizeLadder) {
  const copy = await readFile(
    join(repoRoot, 'openless-all/app/linux-egui/packaging/icons', file),
  );
  const shared = await readFile(
    join(repoRoot, 'openless-all/app/src-tauri/icons', file),
  );
  assert.ok(
    copy.equals(shared),
    `linux-egui packaging icon ${file} must stay byte-identical to the shared src-tauri/icons copy`,
  );
  assert.ok(
    packageScript.includes(`${hicolor}:${file}`),
    `package script must install ${file} into hicolor/${hicolor}`,
  );
}
assert.ok(
  packageScript.includes('usr/share/icons/hicolor/${spec%%:*}/apps/openless.png'),
  'package script must install the whole hicolor ladder, not a single size',
);

// 10. 发版编排里的 Linux 腿必须是**内联的普通 job**（用户要求：Actions 页面上与
//     三个平台平级，不能是可复用工作流那种嵌套折叠显示），而且手动安装 zip 必须
//     在同一个 job 里产出（不是单独的 bundle job）。
const orchestratedWorkflow = await readFile(
  join(repoRoot, '.github/workflows/release-tauri.yml'),
  'utf8',
);
assert.ok(!orchestratedWorkflow.includes('uses: ./.github/workflows/release-linux-egui.yml'),
  'Linux leg must be inlined so Actions does not nest it under a called workflow');
assert.ok(orchestratedWorkflow.includes('name: Linux egui packages (deb + rpm + manual zip)'),
  'orchestrator must carry the inlined Linux job');
assert.ok(!/bundle-manual-install/.test(orchestratedWorkflow),
  'the manual zip must be built inside the Linux job, not a separate bundle job');

// 两份副本的关键门禁必须一致，否则内联版会悄悄漂移成弱门禁。
for (const gate of [
  'test "$(find "$OUTPUT" -maxdepth 1 -name \'*.deb\' | wc -l)" -eq 1',
  'test "$(find "$OUTPUT" -maxdepth 1 -name \'*.rpm\' | wc -l)" -eq 1',
  "! ldd target/release/openless-linux-egui | grep -q 'not found'",
  'openless-all/app/scripts/linux-egui-manual-install.sh',
  'bash -n manual/install.sh',
  'usr/lib/x86_64-linux-gnu/fcitx5/libopenless.so',
  'dpkg-deb -x',
]) {
  assert.ok(orchestratedWorkflow.includes(gate),
    `inlined Linux job must keep the same gate as the standalone entry: ${gate}`);
}

// 11. 统一发布语义：任何 v* 发布标签都要出全平台产物（Tauri 三平台 + Linux egui
//     + 安卓），后缀只作命名约定，不能再当「只构建一半」的开关。
const androidWorkflow = await readFile(
  join(repoRoot, '.github/workflows/android-apk.yml'),
  'utf8',
);
assert.ok(/tags:\s*\n\s*- 'v\*'/.test(orchestratedWorkflow),
  'release pipeline must trigger on every v* release tag');
assert.ok(/tags:\s*\n\s*- 'v\*'/.test(androidWorkflow),
  'android workflow must trigger on every v* release tag');
assert.ok(!/endsWith\(github\.ref, '-egui'\)/.test(orchestratedWorkflow),
  'release jobs must not gate on the -egui suffix any more');
assert.ok(androidWorkflow.includes('softprops/action-gh-release'),
  'android assets must land on the same GitHub release as the desktop builds');
// build 与 linux 两个 job 都必须对任何发布标签无条件运行（不再按后缀分流）。
const ungatedJobs = (orchestratedWorkflow.match(/if: \$\{\{ !cancelled\(\) \}\}/g) || []).length;
assert.ok(ungatedJobs >= 2,
  'build and linux jobs must run for every release tag instead of gating on -tauri/-egui');
// Homebrew cask 仍只跟稳定的 -tauri 正式版：这是刻意的分发边界，不能被顺手放开。
assert.ok(orchestratedWorkflow.includes("endsWith(github.ref, '-tauri')"),
  'Homebrew cask must keep updating only for stable -tauri tags');

console.log('linux-egui-release-contract.test.mjs passed');
