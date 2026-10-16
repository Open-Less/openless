import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const read = (path) => readFileSync(new URL('../' + path, import.meta.url), 'utf8');

const adapters = read('src-tauri/src/core_adapters.rs');
assert.match(adapters, /cfg!\(target_os = "macos"\)[^]*?ExperimentalNativeRuntime::new/);
assert.match(
  adapters,
  /store\.models_root_dir\(\)[^]*?default_models_root\(\)[^]*?retire_macos_local_models/,
);
assert.doesNotMatch(adapters, /register\(provider_type[^]*?qwen_cache/);
const router = adapters.slice(
  adapters.indexOf('let native_asr:'),
  adapters.indexOf('let polisher ='),
);
assert.doesNotMatch(router, /LOCAL_QWEN3|LOCAL_WHISPER|Qwen|Whisper/);
assert.match(router, /cfg\(target_os = "macos"\)[^]*?apple_speech::PROVIDER_ID/);
assert.match(router, /apple_speech::AppleSpeechTranscriptionEngine/);
const apple = read('src-tauri/src/asr/apple_speech.rs');
assert.doesNotMatch(apple, /ModelStore|ExperimentalNativeRuntime|download_model/);
assert.match(read('src-tauri/Info.plist'), /NSSpeechRecognitionUsageDescription/);
assert.match(adapters, /cfg\(target_os = "windows"\)\]\s+struct TauriNativeTranscriptionEngine/);
const availability = adapters.slice(
  adapters.indexOf('impl openless_core::ModelRuntimeAdapter'),
  adapters.indexOf('fn inspect_native_models'),
);
assert.match(availability, /LocalAsrRuntime::Generic => false/);

// Foundry is a desktop adapter; mobile must not compile its file-manager calls.
const commands = read('src-tauri/src/commands/mod.rs');
assert.match(commands, /#\[cfg\(not\(mobile\)\)\]\s+mod foundry_asr;/);
assert.match(commands, /#\[cfg\(not\(mobile\)\)\]\s+pub use foundry_asr::\*;/);

const service = read('crates/openless-core/src/experimental_asr.rs');
const run = service.slice(
  service.indexOf('    async fn run('),
  service.indexOf('    pub fn release_runtime('),
);
assert.doesNotMatch(run, /runtime\.prepare/);
assert.match(service, /saved\.download_pending/);
const native = read('src-tauri/src/asr/local/experimental.rs');
assert.match(native, /models\.lease\(model\)[^]*?runtime\.prepare\(model[^]*?models\.check_lease/);

const channels = read('src/pages/settings/ChannelList.tsx');
assert.match(channels, /data-retired-local-asr=/);
assert.match(channels, /disabled=\{retired\}/);
assert.match(channels, /os === 'mac' && isRetiredMacAsrProvider/);
const page = read('src/pages/settings/ExperimentalAsrSection.tsx');
assert.match(page, /experimentalAsr\.memoryHint/);
assert.match(page, /controller/);
const controller = read('src/lib/useExperimentalAsr.ts');
assert.match(controller, /deleteExperimentalAsr/);
const vault = read('src-tauri/src/persistence/credentials.rs');
assert.match(vault, /retire_macos_local_asr\(\)[^]*?ChangeOrigin::LocalOnly/);
console.log('macOS legacy ASR retirement and lazy experimental runtime contracts passed');
