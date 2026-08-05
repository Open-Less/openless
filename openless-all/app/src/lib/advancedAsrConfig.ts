// 通用 OpenAI 兼容 ASR（openai-compatible）与 ZenMux（zenmux）的高级配置。
// JSON 形状与后端 coordinator.rs::AdvancedAsrConfig 保持一致：
// {"verboseJson": bool, "chunkDurationMs": number|null, "enableItn": bool}。
// 解析策略与后端一致：缺失 / 非法一律回落保守默认（不发 response_format、不分片、
// enable_itn 默认开启）。

export interface AdvancedAsrConfig {
  verboseJson: boolean
  chunkDurationMs: number | null
  enableItn: boolean
}

export function parseAdvancedAsrConfig(raw: string | null): AdvancedAsrConfig {
  if (!raw) {
    return { verboseJson: false, chunkDurationMs: null, enableItn: true }
  }
  let value: unknown
  try {
    value = JSON.parse(raw)
  } catch {
    return { verboseJson: false, chunkDurationMs: null, enableItn: true }
  }
  if (typeof value !== 'object' || value === null) {
    return { verboseJson: false, chunkDurationMs: null, enableItn: true }
  }
  const record = value as Record<string, unknown>
  const rawMs = record.chunkDurationMs
  const chunkDurationMs =
    typeof rawMs === 'number' && Number.isFinite(rawMs) && rawMs > 0
      ? Math.floor(rawMs)
      : null
  return {
    verboseJson: record.verboseJson === true,
    chunkDurationMs,
    // 缺失 / 非布尔回落默认开启（与后端 parse_advanced_asr_config 一致）。
    enableItn: record.enableItn !== false,
  }
}

export function serializeAdvancedAsrConfig(config: AdvancedAsrConfig): string {
  return JSON.stringify({
    verboseJson: config.verboseJson,
    chunkDurationMs: config.chunkDurationMs,
    enableItn: config.enableItn,
  })
}
