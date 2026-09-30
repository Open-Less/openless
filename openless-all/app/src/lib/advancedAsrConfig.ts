// Advanced configuration for the generic OpenAI-compatible ASR (openai-compatible) and ZenMux (zenmux).
// JSON shape matches the backend's coordinator.rs::AdvancedAsrConfig:
// {"verboseJson": bool, "chunkDurationMs": number|null, "enableItn": bool}.
// Parsing matches the backend: missing/invalid values always fall back to conservative defaults
// (no response_format, no chunking, enable_itn on by default).

export interface AdvancedAsrConfig {
  verboseJson: boolean;
  chunkDurationMs: number | null;
  enableItn: boolean;
}

export function parseAdvancedAsrConfig(raw: string | null): AdvancedAsrConfig {
  if (!raw) {
    return { verboseJson: false, chunkDurationMs: null, enableItn: true };
  }
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    return { verboseJson: false, chunkDurationMs: null, enableItn: true };
  }
  if (typeof value !== 'object' || value === null) {
    return { verboseJson: false, chunkDurationMs: null, enableItn: true };
  }
  const record = value as Record<string, unknown>;
  const rawMs = record.chunkDurationMs;
  const chunkDurationMs =
    typeof rawMs === 'number' && Number.isFinite(rawMs) && rawMs > 0 ? Math.floor(rawMs) : null;
  return {
    verboseJson: record.verboseJson === true,
    chunkDurationMs,
    // Missing / non-bool falls back to enabled by default (matches the backend's parse_advanced_asr_config).
    enableItn: record.enableItn !== false,
  };
}

export function serializeAdvancedAsrConfig(config: AdvancedAsrConfig): string {
  return JSON.stringify({
    verboseJson: config.verboseJson,
    chunkDurationMs: config.chunkDurationMs,
    enableItn: config.enableItn,
  });
}
