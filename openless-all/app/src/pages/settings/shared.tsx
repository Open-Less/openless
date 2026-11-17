// Setting headings, legacy input styles and display-only provider labels.
import type { CSSProperties, ReactNode } from 'react';
import { Hint } from '../../ui/overlays/Tooltip';

export function SectionTitle({
  children,
  hint,
  style,
}: {
  children: ReactNode;
  /** Functional explanation shown on hovering the title text, for sections whose purpose isn't obvious from the name (e.g. Less Computer). */
  hint?: string;
  style?: CSSProperties;
}) {
  const titleStyle: CSSProperties = {
    fontSize: 14,
    fontWeight: 'var(--ol-font-weight-semibold)',
    color: 'var(--ol-ink)',
    marginBottom: 6,
    letterSpacing: '-0.01em',
    ...style,
  };
  if (!hint) {
    return <div style={titleStyle}>{children}</div>;
  }
  return (
    <div style={titleStyle}>
      <Hint content={hint}>{children}</Hint>
    </div>
  );
}

export function ExperimentalSectionTitle({
  children,
  badge,
  hint,
  style,
}: {
  children: ReactNode;
  badge: string;
  hint?: string;
  style?: CSSProperties;
}) {
  return (
    <SectionTitle hint={hint} style={style}>
      <span style={{ display: 'inline-flex', alignItems: 'center', gap: 7, flexWrap: 'wrap' }}>
        <span>{children}</span>
        <span
          style={{
            padding: '2px 6px',
            borderRadius: 'var(--ol-radius)',
            background: 'var(--ol-blue-soft)',
            color: 'var(--ol-blue)',
            fontSize: 10,
            fontWeight: 'var(--ol-font-weight-semibold)',
            lineHeight: 1.3,
            letterSpacing: 0,
          }}
        >
          {badge}
        </span>
      </span>
    </SectionTitle>
  );
}

// Page slimming: settings page description copy is hidden entirely (component signature + call sites kept for easy restoration).
export function SectionDesc(_props: { children: ReactNode; style?: CSSProperties }) {
  return null;
}

export { SettingRow, Toggle } from '../../ui/controls/SettingRow';

export const btnGhostStyle: CSSProperties = {
  padding: '5px 10px',
  fontSize: 12,
  borderRadius: 'var(--ol-r-sm)',
  border: '0.5px solid var(--ol-line-strong)',
  background: 'var(--ol-control-solid)',
  color: 'var(--ol-ink-2)',
  cursor: 'default',
  fontFamily: 'inherit',
  maxWidth: '100%',
  transition: 'background 0.16s var(--ol-motion-quick), border-color 0.16s var(--ol-motion-quick)',
};

export const inputStyle: CSSProperties = {
  flex: 1,
  height: 32,
  padding: '0 10px',
  border: '0.5px solid var(--ol-line-strong)',
  borderRadius: 'var(--ol-control-radius)',
  fontSize: 13.5,
  fontFamily: 'inherit',
  outline: 'none',
  // Same background as the SelectLite trigger: previously --ol-surface-2 (light gray) made every input /
  // dropdown inconsistent with other settings controls (mic / capsule style, etc., which use select-trigger-bg).
  background: 'var(--ol-select-trigger-bg)',
  width: '100%',
  maxWidth: 360,
  transition: 'background 0.16s var(--ol-motion-quick), border-color 0.16s var(--ol-motion-quick)',
};

// React keeps only display labels here. endpoint, model, auth, probe, and capabilities all come from
// the Core ProviderDescriptor so no Host owns a drifting copy of provider policy. Order and keys here
// are only for localization fallback; protocol notes belong near Core provider_rules.
export const ASR_LABELS = [
  { id: 'experimental-textstream', nameKey: 'asrExperimentalTextStream' },
  { id: 'experimental-sensevoice', nameKey: 'asrExperimentalSenseVoice' },
  { id: 'experimental-embedding-gemma', nameKey: 'asrExperimentalGemma' },
  { id: 'volcengine', nameKey: 'asrVolcengine' },
  { id: 'elevenlabs', nameKey: 'asrElevenLabs' },
  { id: 'bailian', nameKey: 'asrBailian' },
  { id: 'bailian-qwen3-realtime', nameKey: 'asrBailianQwen3' },
  { id: 'bailian-fun-asr-flash', nameKey: 'asrBailianFunAsrFlash' },
  { id: 'siliconflow', nameKey: 'asrSiliconflow' },
  { id: 'stepfun', nameKey: 'asrStepfun' },
  { id: 'zhipu', nameKey: 'asrZhipu' },
  { id: 'minimax', nameKey: 'asrMinimax' },
  { id: 'groq', nameKey: 'asrGroq' },
  { id: 'whisper', nameKey: 'asrWhisper' },
  { id: 'openrouter', nameKey: 'asrOpenrouter' },
  { id: 'orcarouter', nameKey: 'orcarouter' },
  { id: 'zenmux', nameKey: 'asrZenmux' },
  { id: 'openai-compatible', nameKey: 'asrOpenAiCompatible' },
  { id: 'xiaomi-mimo-asr', nameKey: 'asrXiaomiMimo' },
  { id: 'iflytek', nameKey: 'asrIflytek' },
  { id: 'tencent-cloud', nameKey: 'asrTencentCloud' },
  { id: 'gemini', nameKey: 'asrGemini' },
  { id: 'foundry-local-whisper', nameKey: 'asrFoundryLocalWhisper' },
  { id: 'local-whisper', nameKey: 'asrLocalWhisper' },
  { id: 'sherpa-onnx-local', nameKey: 'asrSherpaOnnxLocal' },
  { id: 'experimental-sensevoice', nameKey: 'asrExperimentalSenseVoice' },
  { id: 'experimental-embedding-gemma', nameKey: 'asrExperimentalGemma' },
  { id: 'local-qwen3-mlx', nameKey: 'asrLocalQwen3Mlx' },
  { id: 'local-qwen3-c', nameKey: 'asrLocalQwen3C' },
  { id: 'local-qwen3', nameKey: 'asrLocalQwen3' },
  { id: 'apple-speech', nameKey: 'asrAppleSpeech' },
] as const;
