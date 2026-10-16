import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const read = (p) => readFileSync(new URL('../' + p, import.meta.url), 'utf8');
const overlay = JSON.parse(read('src-tauri/tauri.macos.conf.json'));
assert.equal(overlay.bundle.macOS.files, undefined, 'no legacy metallib in the main app');
assert.equal(overlay.build?.beforeBundleCommand, undefined);
const build = read('scripts/build-mac.sh');
assert.match(build, /tauri\.macos\.conf\.json/);
assert.doesNotMatch(build, /stage-macos-mlx|check:macos-metal|KEEP_QWEN_DIR/);
assert.doesNotMatch(build, /--bundles app|codesign --force|hdiutil create/);
assert.match(build, /stapler validate/);
assert.match(build, /包含已下架的 MLX 运行资源/);
assert.equal(
  JSON.parse(read('src-tauri/tauri.conf.json')).build.beforeBuildCommand,
  'npm run build',
);
// Apple Speech is a system service, independent of the retired MLX runtime.
assert.match(read('src-tauri/Info.plist'), /NSSpeechRecognitionUsageDescription/);
assert.match(read('src-tauri/build.rs'), /cargo:rustc-link-lib=framework=Speech/);
assert.match(build, /check-macos-audio-usage-description/);
console.log('macOS optional runtime bundle contracts passed');
