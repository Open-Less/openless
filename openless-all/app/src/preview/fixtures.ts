import type { ActivityDay, DictationSession } from '../api/contracts';
import { OL_DATA } from '../lib/mockData';

export const previewHistoryEntries: DictationSession[] = OL_DATA.history.map((h, i) => ({
  id: `mock-${i}`,
  source: 'voice',
  createdAt: new Date().toISOString(),
  rawTranscript: h.preview,
  asrTranscript: null,
  finalText: h.preview,
  mode: 'structured',
  stylePackId: 'builtin.structured',
  translationActive: false,
  polishSource: null,
  appBundleId: null,
  appName: 'VS Code',
  insertStatus: 'inserted',
  errorCode: null,
  durationMs: 600,
  dictionaryEntryCount: 28,
  hasAudioRecording: null,
  // Rotate three profiles covering the shapes UI acceptance looks at: sub-second streaming
  // finish (millisecond precision), volc resource id, very long provider/model text wrapping;
  // i%4==3 simulates Raw passthrough (no LLM row) and older entries missing duration.
  asrProvider: ['bailian-qwen3-realtime', 'volcengine', 'openrouter', 'apple-speech'][i % 4],
  asrModel: [
    'qwen3-asr-flash-realtime',
    'volc.seedasr.sauc.duration',
    'openai/whisper-large-v3-turbo-preview-2026-01-31',
    null,
  ][i % 4],
  llmProvider: ['ark', 'codex_oauth', 'openrouter', null][i % 4],
  llmModel: [
    'deepseek-v3-2',
    'gpt-5.5-codex-spark',
    'anthropic/claude-sonnet-5-20260203-preview-long-context',
    null,
  ][i % 4],
  asrMs: [120, 64, 9840, null][i % 4],
  polishMs: [1240, 890, 12400, null][i % 4],
}));

previewHistoryEntries.push({
  ...previewHistoryEntries[0],
  id: 'mock-quick-note',
  source: 'quick_note',
  durationMs: 1200,
  rawTranscript: 'Remember to bring the notes to the planning meeting.',
  finalText: 'Bring the notes to the planning meeting.',
  appName: null,
  insertStatus: 'notRequested',
  hasAudioRecording: true,
});

export const previewActivityDays: ActivityDay[] = (() => {
  const days: ActivityDay[] = [];
  const today = new Date();
  for (let i = 364; i >= 0; i -= 1) {
    const d = new Date(today);
    d.setDate(today.getDate() - i);
    const seed = Math.abs(Math.sin(i * 12.9898) * 43758.5453) % 1;
    if (seed < 0.55) continue;
    const count = Math.max(1, Math.round(seed * 22) - 8);
    const iso = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
    // Chars/duration derive at ~120 chars and ~9s per entry so the periodic metric cards have
    // visible data under browser dev too. The oldest 30 days intentionally carry only count
    // (no chars/durationMs), simulating pre-upgrade data and verifying that "old dates show 0
    // in the chars/duration metrics" doesn't crash.
    const legacy = i > 334;
    days.push(
      legacy
        ? { date: iso, count }
        : {
            date: iso,
            count,
            chars: count * (90 + Math.round(seed * 70)),
            durationMs: count * (6000 + Math.round(seed * 7000)),
          },
    );
  }
  return days;
})();
