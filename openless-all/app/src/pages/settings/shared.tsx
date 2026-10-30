// Display atoms (SettingRow / Toggle / inputStyle) and pure i18n label tables shared across settings sections.

import { createContext, useContext, useId, type CSSProperties, type ReactNode } from 'react';
import { toggleGroupFeedback } from '../../lib/toggleFeedback';
import { Switch } from '@base-ui/react/switch';
const SettingLabel = createContext<string | undefined>(undefined);
import { Hint } from '../../components/Tooltip';
import {
  useMobileLayout,
  useReadableLayout,
  useConservativeLayout,
} from '../../lib/useMobileLayout';

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
            borderRadius: 999,
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

interface SettingRowProps {
  label: string;
  desc?: ReactNode;
  children: ReactNode;
  controlWidth?: number | string;
  className?: string;
}

// Explanations share the same dotted-label trigger, including keyboard and touch access.
export function SettingRow({ label, desc, children, controlWidth, className }: SettingRowProps) {
  const labelId = useId();
  const mobile = useMobileLayout();
  const readable = useReadableLayout();
  const conservative = useConservativeLayout();
  const stackLayout = mobile || readable || conservative;
  const labelStyle: CSSProperties = {
    fontSize: 14,
    fontWeight: 'var(--ol-font-weight-medium)',
    color: 'var(--ol-ink)',
    minWidth: 0,
  };
  return (
    <div
      className={['ol-setting-row', className].filter(Boolean).join(' ')}
      style={{
        display: 'grid',
        gridTemplateColumns: stackLayout ? 'minmax(0, 1fr)' : 'minmax(0, 200px) minmax(0, 1fr)',
        gap: stackLayout ? 8 : 16,
        padding: stackLayout ? '12px 0' : '14px 0',
        borderTop: '0.5px solid var(--ol-line-soft)',
        alignItems: 'center',
      }}
    >
      <div style={{ minWidth: 0, alignSelf: 'center' }}>
        <div style={labelStyle}>
          {desc ? (
            <Hint id={labelId} content={desc}>
              {label}
            </Hint>
          ) : (
            <span id={labelId}>{label}</span>
          )}
        </div>
      </div>
      <div
        className="ol-flex-row"
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'flex-start',
          minWidth: 0,
          width: stackLayout ? '100%' : (controlWidth ?? 'auto'),
          maxWidth: '100%',
          flexWrap: stackLayout ? 'wrap' : 'nowrap',
          gap: stackLayout ? 6 : undefined,
        }}
      >
        <SettingLabel.Provider value={labelId}>{children}</SettingLabel.Provider>
      </div>
    </div>
  );
}

export function Toggle({
  on,
  onToggle,
  disabled = false,
  label,
}: {
  on: boolean;
  onToggle?: (next: boolean) => void;
  disabled?: boolean;
  label?: string;
}) {
  const labelId = useContext(SettingLabel);
  return (
    <Switch.Root
      className="ol-switch"
      checked={on}
      onCheckedChange={(next) => onToggle?.(next)}
      aria-label={label}
      aria-labelledby={label ? undefined : labelId}
      disabled={disabled}
      onClick={(event) => {
        if (disabled) return;
        if (event.detail > 0) toggleGroupFeedback(event.currentTarget);
      }}
    >
      {/* Geometry, colours and motion live in .ol-switch (refinements.css). Width and
          height are locked there, so a column-direction row can't stretch it into a ball. */}
      <Switch.Thumb className="ol-switch-thumb" />
    </Switch.Root>
  );
}

export const btnGhostStyle: CSSProperties = {
  padding: '5px 10px',
  fontSize: 12,
  borderRadius: 6,
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
  borderRadius: 8,
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
