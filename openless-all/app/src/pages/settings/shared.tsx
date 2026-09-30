// Display atoms (SettingRow / Toggle / inputStyle) and pure i18n label tables shared across settings sections.

import type { CSSProperties, ReactNode } from 'react';
import { Icon } from '../../components/Icon';
import { Tooltip } from '../../components/Tooltip';
import {
  useMobileLayout,
  useReadableLayout,
  useConservativeLayout,
} from '../../lib/useMobileLayout';

// Hintable text gets a dotted underline + help cursor, signaling "hover for an explanation".
const hintableTextStyle: CSSProperties = {
  cursor: 'help',
  textDecoration: 'underline dotted',
  textDecorationColor: 'var(--ol-ink-4)',
  textUnderlineOffset: 3,
};

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
    fontWeight: 600,
    color: 'var(--ol-ink)',
    marginBottom: 6,
    letterSpacing: '-0.01em',
    ...style,
  };
  if (!hint) {
    return <div style={titleStyle}>{children}</div>;
  }
  return (
    // display:flex shrinks the Tooltip anchor to the title text itself so the tooltip pops up next to the words.
    <div style={{ ...titleStyle, display: 'flex' }}>
      <Tooltip content={hint} wrap placement="bottom" focusable>
        <span style={hintableTextStyle}>{children}</span>
      </Tooltip>
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
            fontWeight: 600,
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
  desc?: string;
  children: ReactNode;
  controlWidth?: number | string;
  className?: string;
}

// A setting's purpose and consequences read directly; touch and keyboard users don't depend on hover.
export function SettingRow({ label, desc, children, controlWidth, className }: SettingRowProps) {
  const mobile = useMobileLayout();
  const readable = useReadableLayout();
  const conservative = useConservativeLayout();
  const stackLayout = mobile || readable || conservative;
  const labelStyle: CSSProperties = {
    fontSize: 14,
    fontWeight: 500,
    color: 'var(--ol-ink)',
    minWidth: 0,
  };
  return (
    <div
      className={className}
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
        {/* Inline long descriptions no longer occupy space; they collapse into the "?" next to the title,
                    popping up on hover / click (same interaction language as the "Recording & Input" title hint). */}
        <div style={{ display: 'flex', alignItems: 'center', gap: 6, minWidth: 0 }}>
          <span style={{ ...labelStyle, minWidth: 0 }}>{label}</span>
          {desc && (
            <Tooltip content={desc} wrap placement="bottom" focusable>
              <span
                aria-label={desc}
                style={{
                  display: 'inline-flex',
                  alignItems: 'center',
                  justifyContent: 'center',
                  width: 16,
                  height: 16,
                  borderRadius: 999,
                  border: '0.5px solid var(--ol-line-strong)',
                  color: 'var(--ol-ink-4)',
                  cursor: 'help',
                  flexShrink: 0,
                }}
              >
                <Icon name="help" size={10} />
              </span>
            </Tooltip>
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
        {children}
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
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      aria-label={label}
      disabled={disabled}
      onClick={() => {
        if (!disabled) onToggle?.(!on);
      }}
      style={{
        position: 'relative',
        // Previously hardcoded flex: 0 0 36px; in the column-direction settings row wrapper flex-basis
        // applied to height, stretching the toggle into a 36×36 ball (the launch-at-login row).
        // Width/height are locked explicitly; flex only prevents growing/shrinking.
        flex: '0 0 auto',
        width: 36,
        minWidth: 36,
        maxWidth: 36,
        height: 20,
        borderRadius: 999,
        border: 0,
        background: on ? 'var(--ol-blue)' : 'var(--ol-toggle-off-bg)',
        boxShadow: 'inset 0 1px 2px rgba(0,0,0,0.06)',
        cursor: disabled ? 'not-allowed' : 'pointer',
        opacity: disabled ? 0.45 : 1,
        transition: 'background 0.16s var(--ol-motion-quick)',
      }}
    >
      <span
        style={{
          position: 'absolute',
          top: 2,
          left: on ? 18 : 2,
          width: 16,
          height: 16,
          borderRadius: 999,
          background: 'var(--ol-toggle-knob)',
          boxShadow: '0 1px 2px rgba(0,0,0,.25), 0 0 0 0.5px rgba(0,0,0,.04)',
          transition: 'left .16s var(--ol-motion-spring)',
        }}
      />
    </button>
  );
}

export function chipSelectedStyle(selected: boolean): CSSProperties {
  return {
    background: selected ? 'var(--ol-pill-selected-bg)' : 'transparent',
    border: selected
      ? '0.5px solid var(--ol-pill-selected-border)'
      : '0.5px solid var(--ol-line-strong)',
    color: selected ? 'var(--ol-pill-selected-ink)' : 'var(--ol-ink-3)',
  };
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

export const segmentedTrackStyle: CSSProperties = {
  display: 'inline-flex',
  padding: 2,
  borderRadius: 8,
  background: 'var(--ol-segmented-bg)',
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
  { id: 'foundry-local-whisper', nameKey: 'asrFoundryLocalWhisper' },
  { id: 'local-whisper', nameKey: 'asrLocalWhisper' },
  { id: 'sherpa-onnx-local', nameKey: 'asrSherpaOnnxLocal' },
  { id: 'local-qwen3-mlx', nameKey: 'asrLocalQwen3Mlx' },
  { id: 'local-qwen3-c', nameKey: 'asrLocalQwen3C' },
  { id: 'local-qwen3', nameKey: 'asrLocalQwen3' },
  { id: 'apple-speech', nameKey: 'asrAppleSpeech' },
] as const;
