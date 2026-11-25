// Advanced configuration for openai-compatible, zenmux, and volcengine ASR.
// JSON shape matches the backend's provider_rules.rs::AdvancedAsrConfig:
// {"verboseJson": bool, "chunkDurationMs": number|null, "enableItn": bool, "enableDdc": bool, "enableNonstream": bool}.
// Parsing matches the backend: missing/invalid values always fall back to conservative defaults.

export interface AdvancedAsrConfig {
  verboseJson: boolean;
  chunkDurationMs: number | null;
  enableItn: boolean;
  enableDdc: boolean;
  enableNonstream: boolean;
}

const DEFAULT_CONFIG: AdvancedAsrConfig = {
  verboseJson: false,
  chunkDurationMs: null,
  enableItn: true,
  enableDdc: false,
  enableNonstream: false,
};

export function parseAdvancedAsrConfig(raw: string | null): AdvancedAsrConfig {
  if (!raw) {
    return { ...DEFAULT_CONFIG };
  }
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    return { ...DEFAULT_CONFIG };
  }
  if (typeof value !== 'object' || value === null) {
    return { ...DEFAULT_CONFIG };
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
    enableDdc: record.enableDdc === true,
    enableNonstream: record.enableNonstream === true,
  };
}

export function serializeAdvancedAsrConfig(config: AdvancedAsrConfig): string {
  return JSON.stringify({
    verboseJson: config.verboseJson,
    chunkDurationMs: config.chunkDurationMs,
    enableItn: config.enableItn,
    enableDdc: config.enableDdc,
    enableNonstream: config.enableNonstream,
  });
}
