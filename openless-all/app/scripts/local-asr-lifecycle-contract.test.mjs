import assert from 'node:assert/strict';
import { readFileSync, existsSync } from 'node:fs';
const read = (p) => readFileSync(new URL('../' + p, import.meta.url), 'utf8');
const cargo = read('src-tauri/Cargo.toml');
assert.doesNotMatch(cargo, /qwen3-asr-rs\s*=|whisper-rs\s*=|tokenizers\s*=/);
const modules = read('src-tauri/src/asr/local/mod.rs');
assert.doesNotMatch(modules, /mod mlx_worker|mod qwen_engine|mod whisper_provider|mod apple_speech_provider/);
assert.doesNotMatch(modules, /mod retired|QwenBackend|mod models/);
assert.equal(existsSync(new URL('../src-tauri/src/asr/local/retired.rs', import.meta.url)), false);
const native = read('src-tauri/src/core_adapters.rs');
assert.doesNotMatch(native, /LocalAsrCache|LocalWhisperCache|LocalQwenEngine|AppleSpeechAsr/);
const worker = read('src-tauri/src/asr/local/experimental/textstream.rs');
assert.match(worker, /installed\(&directory\)[^]*?verify_package[^]*?models\.check_lease[^]*?Command::new\(install::python/);
assert.match(worker, /TextStreamChunk\s*\{\s*text: finalized \+ &draft,\s*offset: 0/);
const core = read('crates/openless-core/src/local_asr_service.rs');
const activate = core.match(/    fn activate\([^]*?(?=\n    fn )/)[0];
assert.doesNotMatch(activate, /\.(?:prepare|preload|preload_lease|download_native_model)\(/);
const page = read('src/pages/LocalAsr/index.tsx');
for (const name of ['handleDownload', 'handleDownloadSherpa', 'handlePrepareFoundry']) {
 const body = page.match(new RegExp('const ' + name + ' = async[^]*?(?=\\n  const )'))[0];
 assert.doesNotMatch(body, /activateLocalAsr|activateSherpaProvider|preloadLocalAsr|handleEnable/);
}
assert.doesNotMatch(read('src/pages/settings/models/LocalModelPicker.tsx'), /preloadLocalAsr/);
console.log('local inference lifecycle and optional runtime isolation contracts passed');
