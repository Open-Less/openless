import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

function sliceBetween(source, start, end, name) {
  const startIndex = source.indexOf(start);
  assert.notEqual(startIndex, -1, `${name}: missing start marker ${start}`);
  const endIndex = source.indexOf(end, startIndex + start.length);
  assert.notEqual(endIndex, -1, `${name}: missing end marker ${end}`);
  return source.slice(startIndex, endIndex);
}

function assertOrdered(source, fragments, name) {
  let cursor = -1;
  for (const fragment of fragments) {
    const next = source.indexOf(fragment, cursor + 1);
    assert.notEqual(next, -1, `${name}: missing or out of order fragment ${fragment}`);
    cursor = next;
  }
}

const localProviderRs = await readFile(
  new URL("../src-tauri/src/asr/local/local_provider.rs", import.meta.url),
  "utf8",
);
const coordinatorRs = await readFile(
  new URL("../src-tauri/src/coordinator.rs", import.meta.url),
  "utf8",
);
const dictationRs = await readFile(
  new URL("../src-tauri/src/coordinator/dictation.rs", import.meta.url),
  "utf8",
);

const localQwenImpl = sliceBetween(
  localProviderRs,
  "impl LocalQwenAsr {",
  "impl crate::recorder::AudioConsumer for LocalQwenAsr {",
  "LocalQwenAsr impl",
);

assertOrdered(
  localQwenImpl,
  [
    "pub fn buffer_duration_ms(&self) -> u64 {",
    "(self.buffer.lock().len() as u64 / 2) * 1000 / 16_000",
    "pub async fn transcribe(self: Arc<Self>) -> Result<RawTranscript> {",
    "let pcm_bytes = std::mem::take(&mut *self.buffer.lock());",
    "let duration_ms = (pcm_bytes.len() as u64 / 2) * 1000 / 16_000;",
    "let mut samples_f32 = i16_le_bytes_to_f32(&pcm_bytes);",
    "samples_f32.extend(std::iter::repeat(0.0f32).take(8_000));",
    "engine.transcribe_stream(&samples_f32)",
    "Ok(RawTranscript { text, duration_ms })",
  ],
  "local Qwen ASR should measure original audio, append 0.5s silence, and transcribe padded samples",
);

const localQwenBranch = sliceBetween(
  dictationRs,
  "ActiveAsr::Local(local) => {",
  "inner.local_asr_cache.touch();",
  "dictation local Qwen branch",
);
assertOrdered(
  localQwenBranch,
  [
    "let audio_secs = (local.buffer_duration_ms() as f64) / 1000.0;",
    "let timeout_duration = local_qwen_transcribe_timeout(audio_secs);",
    "\"[coord] local Qwen3-ASR transcribe: audio={:.2}s timeout={}s\"",
    "let result = tokio::time::timeout(timeout_duration, local.transcribe()).await;",
  ],
  "dictation should compute a dynamic timeout from buffered audio before consuming local Qwen ASR",
);

const timeoutHelper = sliceBetween(
  coordinatorRs,
  "fn local_qwen_transcribe_timeout(audio_secs: f64) -> std::time::Duration {",
  "fn startup_race_status_for_starting(",
  "local_qwen_transcribe_timeout",
);
assertOrdered(
  timeoutHelper,
  [
    "let secs = ((audio_secs * 0.6).ceil() as u64)",
    ".saturating_add(10)",
    ".max(COORDINATOR_GLOBAL_TIMEOUT_SECS);",
    "std::time::Duration::from_secs(secs)",
  ],
  "local Qwen ASR timeout should be max(15, ceil(audio_s * 0.6) + 10)",
);

for (const testName of [
  "local_qwen_timeout_floors_at_global_timeout_for_short_audio",
  "local_qwen_timeout_scales_with_audio_duration",
  "local_qwen_timeout_ceils_partial_seconds",
  "local_qwen_timeout_handles_zero_duration",
]) {
  assert.match(coordinatorRs, new RegExp(`fn ${testName}\\(\\)`), `missing Rust timeout test ${testName}`);
}
