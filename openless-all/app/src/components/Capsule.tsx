import { Icon } from './Icon';
import { memo, useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { detectOS, type OS } from './WindowChrome';
import { warmUpSiriShaders } from './SiriGL';
import { VoiceOrbStage } from './VoiceOrbStage';
import { TypelessCapsule } from './TypelessCapsule';
import { CapsuleTranscriptRail, CapsuleTranslationBadge } from './LiveTranscriptPill';
import { capsuleTranscriptFontSize, visibleCapsuleTranscript } from '../lib/capsuleTranscript';
import { getSettings } from '../lib/ipc/settings';
import { cancelDictation, setCapsuleTranscriptVisible, stopDictation } from '../lib/ipc/dictation';
import {
  getCapsuleHostMetrics,
  getCapsuleMessageLayout,
  getCapsulePillMetrics,
  getClassicCapsuleGeometry,
  getClassicProcessingLabel,
  getCapsuleTranscriptRailHeight,
  parseCapsuleStyle,
} from '../lib/capsuleLayout';
import { isTauri } from '../lib/ipc';
import type {
  CapsulePayload,
  CapsuleState,
  CapsuleStyle,
  InsertFallbackCardPayload,
  PendingCorrection,
  UserPreferences,
} from '../lib/types';
import { VocabSuggestionCard } from './VocabSuggestionCard';
import { InsertFallbackCard } from './InsertFallbackCard';
import {
  applyTranscriptEvent,
  beginTranscriptGeneration,
  clearTranscriptText,
  createTranscriptViewState,
  type BackendEvent,
  type TranscriptViewState,
} from '../lib/backendEvent';

// Inject the capsule keyframes once into document.head instead of rendering them in
// JSX: per-frame (~60Hz) setLevel during recording would otherwise recreate this
// static <style> element on every render. Same approach as QaPanel / LessComputerPanel.
const CAPSULE_KEYFRAMES = `
  @keyframes capsule-in {
    from { opacity: 0; transform: scale(.46) translateY(18px); }
    70%  { opacity: 1; transform: scale(1.035) translateY(-1px); }
    to   { opacity: 1; transform: scale(1)    translateY(0); }
  }
  @keyframes capsule-out {
    from { opacity: 1; transform: scale(1)   translateY(0); }
    to   { opacity: 0; transform: scale(.46) translateY(18px); }
  }
  /* Thinking dots (fluid dots) entrance: pure fade-in that catches the light orb —
     the dots' own "spread from center" motion is driven by the shader's uGather,
     so no scale punch here. */
  @keyframes siri-orb-in {
    from { opacity: 0; }
    to   { opacity: 1; }
  }
  @keyframes selection-polish-spinner {
    to { transform: rotate(360deg); }
  }
  /* Classic pill (Openless default style) only:
     - cap-shine: blue light sweep over the "thinking" text during transcribe/polish;
     - cap-state-enter: fade-in when state content switches. */
  @keyframes cap-shine {
    0%   { background-position: 200% center; }
    100% { background-position: -200% center; }
  }
  @keyframes cap-state-enter {
    from { opacity: 0; transform: translateY(2px); }
    to   { opacity: 1; transform: translateY(0); }
  }
`;

if (typeof document !== 'undefined' && !document.getElementById('capsule-keyframes')) {
  const tag = document.createElement('style');
  tag.id = 'capsule-keyframes';
  tag.textContent = CAPSULE_KEYFRAMES;
  document.head.appendChild(tag);
}

interface SelectionPolishNoticeProps {
  state: CapsuleState;
  message?: string;
}

/**
 * Non-interactive one-line notice for selection polish. Lives in the same Windows
 * no-activate + mouse-passthrough capsule window, so it never steals focus from the
 * user's input field and never blocks clicks.
 */
function SelectionPolishNotice({ state, message }: SelectionPolishNoticeProps) {
  const { t } = useTranslation();
  const processing = state === 'polishing';
  const failed = state === 'error';
  const completed = state === 'done';
  const cancelled = state === 'cancelled';
  const label =
    message ??
    (processing
      ? t('capsule.selectionPolish.polishing')
      : completed
        ? t('capsule.selectionPolish.replaced')
        : cancelled
          ? t('capsule.selectionPolish.noSelection')
          : t('capsule.selectionPolish.failed'));
  const color = failed
    ? '#ff8278'
    : completed
      ? '#63d596'
      : cancelled
        ? 'var(--ol-capsule-center-ink)'
        : '#8fc0ff';
  const symbol = completed ? '✓' : failed ? '!' : cancelled ? '·' : null;

  return (
    <div
      role={failed ? 'alert' : 'status'}
      aria-live="polite"
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 8,
        maxWidth: 360,
        padding: '8px 14px',
        borderRadius: 999,
        border: failed
          ? '1px solid rgba(255, 112, 102, 0.42)'
          : '1px solid var(--ol-capsule-pill-border)',
        background: 'var(--ol-capsule-pill-bg)',
        boxShadow: 'var(--ol-capsule-pill-shadow)',
        color,
        fontFamily: 'var(--ol-font-sans)',
        fontSize: 13,
        fontWeight: 600,
        lineHeight: 1.25,
        letterSpacing: '0.01em',
        pointerEvents: 'none',
      }}
    >
      {processing ? (
        <span
          aria-hidden="true"
          style={{
            width: 12,
            height: 12,
            flex: '0 0 auto',
            border: '2px solid rgba(143, 192, 255, 0.32)',
            borderTopColor: color,
            borderRadius: '50%',
            animation: 'selection-polish-spinner .75s linear infinite',
          }}
        />
      ) : (
        <span
          aria-hidden="true"
          style={{
            display: 'inline-flex',
            width: 13,
            justifyContent: 'center',
            flex: '0 0 auto',
            color,
            fontSize: completed ? 15 : 16,
            fontWeight: 800,
            lineHeight: 1,
          }}
        >
          {symbol}
        </span>
      )}
      <span
        style={{
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          whiteSpace: 'nowrap',
        }}
      >
        {label}
      </span>
    </div>
  );
}

// ───────── Classic pill (Openless default style) ────────────────────────
// Restored from the 1.3.14 capsule: frosted pill + volume bars + cancel/confirm
// buttons. Sizes match the capsuleLayout constants of that release (the Siri orb
// stage is full-bleed 460×180; the classic pill is a small centered pill).

interface ClassicPillMetrics {
  width: number;
  height: number;
  textWidth: number;
}

const CLASSIC_PILL_METRICS: ClassicPillMetrics = {
  width: 176,
  height: 42,
  textWidth: 84,
};
const CLASSIC_PILL_METRICS_WIN: ClassicPillMetrics = {
  width: 196,
  height: 52,
  textWidth: 104,
};

function classicPillMetrics(os: OS): ClassicPillMetrics {
  return os === 'win' ? CLASSIC_PILL_METRICS_WIN : CLASSIC_PILL_METRICS;
}

function AudioBars({ level, compact = false }: { level: number; compact?: boolean }) {
  const envelope = [0.55, 0.85, 1.0, 0.85, 0.55];
  const base = 2;
  const max = compact ? 16 : 24;
  const voice = Math.min(1, Math.max(0, level));
  const silenceGate = 0.012;
  const responseCeiling = 0.34;
  const gatedVoice = Math.min(
    1,
    Math.max(0, (voice - silenceGate) / (responseCeiling - silenceGate)),
  );
  const easedVoice = gatedVoice * gatedVoice * (3 - 2 * gatedVoice);
  const visualVoice = Math.pow(easedVoice, 0.42);
  return (
    <div
      className="ol-capsule-audio-bars"
      style={{
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        gap: compact ? 2 : 3,
        width: compact ? 24 : 42,
        height: max,
      }}
    >
      {envelope.map((env, i) => (
        <span
          key={i}
          style={{
            display: 'inline-block',
            width: compact ? 2 : 3,
            height: base + (max - base) * visualVoice * env,
            borderRadius: 999,
            background: 'var(--ol-blue)',
            opacity: 0.82,
            transformOrigin: 'center',
            // 0.08s is too fast at 60Hz audio-level updates: every re-render restarts
            // the transition, reading as stepped jumps. 0.18s lets successive updates
            // blend smoothly within the curve; easeOutExpo-like easing keeps the
            // dot→bar morph natural.
            transition: 'height 0.18s cubic-bezier(0.22, 1, 0.36, 1)',
          }}
        />
      ))}
    </div>
  );
}

interface CenterTextProps {
  os: OS;
  kind: 'default' | 'processing' | 'error';
  text: string;
  color?: string;
}

function CenterText({ os, kind, text, color = 'var(--ol-capsule-center-ink)' }: CenterTextProps) {
  const metrics = classicPillMetrics(os);
  const layout = getCapsuleMessageLayout(os, kind);
  return (
    <span
      style={{
        fontSize: 11,
        fontWeight: 500,
        color,
        width: '100%',
        maxWidth: metrics.textWidth,
        minWidth: 0,
        textAlign: 'center',
        lineHeight: layout.allowWrap ? 1.2 : 1,
        whiteSpace: layout.allowWrap ? 'normal' : 'nowrap',
        overflow: 'hidden',
        textOverflow: 'ellipsis',
        display: '-webkit-box',
        WebkitBoxOrient: 'vertical',
        WebkitLineClamp: layout.lineClamp,
      }}
    >
      {text}
    </span>
  );
}

interface CircleButtonProps {
  variant: 'cancel' | 'confirm';
  enabled: boolean;
  onClick: () => void;
}

// memo: level changes every frame (~60Hz) while recording, re-rendering the pill; the
// cancel/confirm SVG buttons don't depend on level, so memo + stable onClick lets them
// skip re-renders during recording (only the volume bars actually update).
const CircleButton = memo(function CircleButton({ variant, enabled, onClick }: CircleButtonProps) {
  const { t } = useTranslation();
  const isCancel = variant === 'cancel';
  return (
    <button
      onClick={enabled ? onClick : undefined}
      onMouseDown={(event) => {
        event.preventDefault();
        event.stopPropagation();
      }}
      aria-label={isCancel ? t('common.cancel') : t('settings.shortcuts.confirm')}
      disabled={!enabled}
      style={{
        width: 28,
        height: 28,
        borderRadius: 999,
        background: isCancel ? 'var(--ol-capsule-btn-bg)' : 'var(--ol-capsule-btn-bg-confirm)',
        color: 'var(--ol-capsule-btn-ink)',
        border: '0.8px solid var(--ol-capsule-btn-border)',
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'center',
        cursor: enabled ? 'default' : 'not-allowed',
        opacity: enabled ? 1 : 0.42,
        visibility: 'visible',
        flexShrink: 0,
        padding: 0,
        boxShadow: '0 1px 2px rgba(0, 0, 0, 0.06)',
        transition:
          'opacity 0.18s var(--ol-motion-soft), background 0.16s var(--ol-motion-quick), transform 0.12s var(--ol-motion-quick)',
      }}
    >
      <Icon name={isCancel ? 'close' : 'check'} size={13} strokeWidth={2.2} />
    </button>
  );
});

interface ClassicPillProps {
  os: OS;
  state: CapsuleState;
  level: number;
  insertedChars: number;
  message?: string;
  operating: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}

function ClassicPill({
  os,
  state,
  level,
  insertedChars,
  message,
  operating,
  onCancel,
  onConfirm,
}: ClassicPillProps) {
  const { t } = useTranslation();
  const metrics = classicPillMetrics(os);
  const processingLayout = useMemo(() => getCapsuleMessageLayout(os, 'processing'), [os]);
  const cancelEnabled = state === 'recording' || state === 'transcribing' || state === 'polishing';
  const confirmEnabled = state === 'recording';

  // Keep the original Classic processing treatment: the thinking/using label
  // has its own entry and shine animation, while recording keeps the live bars.
  const [shineFast, setShineFast] = useState(true);
  useEffect(() => {
    if (state === 'transcribing' || state === 'polishing') {
      setShineFast(true);
      const timer = setTimeout(() => setShineFast(false), 2000);
      return () => clearTimeout(timer);
    }
    setShineFast(true);
    return undefined;
  }, [state]);

  let center: ReactNode;
  switch (state) {
    case 'recording':
      center = <AudioBars level={level} />;
      break;
    case 'transcribing':
    case 'polishing':
      center = (
        <div
          style={{
            display: 'inline-flex',
            alignItems: 'center',
            gap: 5,
            padding: '0 2px',
            width: '100%',
            maxWidth: metrics.textWidth,
            minWidth: 0,
            justifyContent: 'center',
            animation: 'cap-state-enter 220ms var(--ol-motion-soft) both',
          }}
        >
          <AudioBars level={level} compact />
          <span
            style={{
              fontSize: 11,
              fontWeight: 600,
              letterSpacing: 0.1,
              paddingBlock: 2,
              color: 'var(--ol-ink-2)',
              backgroundImage:
                'linear-gradient(100deg, var(--ol-ink) 0%, var(--ol-ink) 35%, var(--ol-blue) 50%, var(--ol-ink) 65%, var(--ol-ink) 100%)',
              backgroundSize: '220% auto',
              WebkitBackgroundClip: 'text',
              backgroundClip: 'text',
              WebkitTextFillColor: 'transparent',
              animation: `cap-shine ${shineFast ? '0.9s' : '2.4s'} linear infinite`,
              minWidth: 0,
              textAlign: 'center',
              lineHeight: processingLayout.allowWrap ? 1.3 : 1.25,
              whiteSpace: processingLayout.allowWrap ? 'normal' : 'nowrap',
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              display: '-webkit-box',
              WebkitBoxOrient: 'vertical',
              WebkitLineClamp: processingLayout.lineClamp,
            }}
          >
            {t(getClassicProcessingLabel(operating))}
          </span>
        </div>
      );
      break;
    case 'done':
      center = (
        <CenterText
          os={os}
          kind="default"
          text={message || t('capsule.inserted', { count: insertedChars })}
        />
      );
      break;
    case 'cancelled':
      center = <CenterText os={os} kind="default" text={t('capsule.cancelled')} />;
      break;
    case 'error':
      center = (
        <CenterText
          os={os}
          kind="error"
          text={message || t('capsule.error')}
          color="var(--ol-err)"
        />
      );
      break;
    default:
      center = <AudioBars level={0} />;
  }

  const ambient = state === 'recording' ? Math.min(1, Math.max(0, level)) : 0;
  const scale = os === 'win' ? 1 : 1 + ambient * 0.018;
  const shadowAlpha = 0.2 + ambient * 0.1;

  return (
    // Non-Linux uses fake frost; on Linux with transparent windows disabled, .ol-frost
    // platform rules fall back to an opaque surface. No backdrop-filter: the webview
    // can't blur the desktop behind a transparent window (Tauri upstream limitation).
    <div
      className="ol-frost ol-capsule-pill"
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        gap: 4,
        padding: '0 8px',
        width: metrics.width,
        height: metrics.height,
        boxSizing: 'border-box',
        borderRadius: 999,
        border: '1px solid var(--ol-capsule-pill-border)',
        boxShadow: `${os === 'win' ? `0 10px 24px -14px rgba(0, 0, 0, ${(0.24 + ambient * 0.06).toFixed(3)})` : `0 18px 50px -10px rgba(0, 0, 0, ${shadowAlpha.toFixed(3)})`}, 0 0 0 0.5px rgba(0, 0, 0, 0.24), var(--ol-capsule-pill-inset)`,
        color: 'var(--ol-capsule-center-ink)',
        fontFamily: 'var(--ol-font-sans)',
        transform: `scale(${scale.toFixed(4)})`,
        transformOrigin: 'center',
        transition:
          'transform 0.08s var(--ol-motion-quick), box-shadow 0.08s var(--ol-motion-quick)',
        willChange: 'transform, box-shadow',
      }}
    >
      <CircleButton variant="cancel" enabled={cancelEnabled} onClick={onCancel} />
      <div
        style={{
          flex: 1,
          minWidth: 0,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
        }}
      >
        {center}
      </div>
      <CircleButton variant="confirm" enabled={confirmEnabled} onClick={onConfirm} />
    </div>
  );
}

interface ClassicCapsuleProps {
  os: OS;
  state: CapsuleState;
  level: number;
  insertedChars: number;
  message?: string;
  transcript?: string;
  transcriptFontSize?: number;
  transcriptVisible?: boolean;
  transcriptInSameWindow?: boolean;
  operating: boolean;
  translation: boolean;
}

/**
 * Classic pill + "translating" badge (matching 1.3.14). Cancel/confirm call the
 * dictation commands directly (the capsule window never takes focus, so button
 * interaction only appears while recording).
 */
function ClassicCapsule({
  os,
  state,
  level,
  insertedChars,
  message,
  transcript,
  transcriptFontSize = 14,
  transcriptVisible = false,
  transcriptInSameWindow = true,
  operating,
  translation,
}: ClassicCapsuleProps) {
  const hostMetrics = getCapsuleHostMetrics(os, false, 'classic');
  const onCancel = useCallback(() => {
    void cancelDictation();
  }, []);
  const onConfirm = useCallback(() => {
    void stopDictation();
  }, []);
  const liveText = transcript?.trim() ?? '';
  const classicGeometry = getClassicCapsuleGeometry(
    os,
    transcriptVisible || Boolean(liveText),
    translation,
    transcriptInSameWindow,
  );

  return (
    <>
      {/* "Translating" badge — two nested layers:
          the outer layer only does absolute positioning + horizontal centering
          (translateX(-50%)) with no animation; the inner layer only does vertical
          shift + fade. This avoids conflicting with translateX(-50%) and avoids
          keyframe vs inline transform overrides causing visual jumps. */}
      {transcriptInSameWindow && (
        <div
          style={{
            position: 'absolute',
            left: '50%',
            bottom: classicGeometry.badgeBottomOffset,
            transform: 'translateX(-50%)',
            pointerEvents: 'none',
          }}
        >
          <CapsuleTranslationBadge visible={translation} />
        </div>
      )}
      <div className="ol-classic-capsule-stack">
        {liveText && (
          <CapsuleTranscriptRail
            text={liveText}
            fontSize={transcriptFontSize}
            tone="frost"
            stageWidth={hostMetrics.width}
            maxWidth={hostMetrics.width - 16}
            height={getCapsuleTranscriptRailHeight('classic')}
          />
        )}
        <ClassicPill
          os={os}
          state={state}
          level={level}
          insertedChars={insertedChars}
          message={message}
          operating={operating}
          onCancel={onCancel}
          onConfirm={onConfirm}
        />
      </div>
    </>
  );
}

// Must stay in sync with the @keyframes capsule-out duration — otherwise the timer
// unmounts before the animation ends and the user sees it cut off mid-flight. Both
// styles share the same capsule-in/out keyframes with different durations: Siri orb
// 520ms (as-is), classic pill 360ms (matching 1.3.14).
const EXIT_ANIM_MS_SIRI = 520;
const EXIT_ANIM_MS_CLASSIC = 360;

// Average capsule warm→ready duration (ms), driving the predicted pace of the entry
// expand animation (see SiriGL warmProgress): EMA-updated after each recording becomes
// ready and persisted to localStorage across sessions. Default 150ms, clamped [60,600].
const WARMUP_MS_KEY = 'ol-capsule-warmup-ms';
const WARMUP_MS_DEFAULT = 150;
function readWarmupMs(): number {
  if (typeof localStorage === 'undefined') return WARMUP_MS_DEFAULT;
  const raw = Number(localStorage.getItem(WARMUP_MS_KEY));
  return Number.isFinite(raw) && raw > 0 ? Math.min(600, Math.max(60, raw)) : WARMUP_MS_DEFAULT;
}
// #470 diagnostics v2: module-level one-shot gate; log only when the webview receives
// its first capsule:state event.
let capsuleStateFirstLogged = false;

const CAPSULE_PREVIEW_STATES: CapsuleState[] = [
  'idle',
  'recording',
  'transcribing',
  'polishing',
  'done',
  'cancelled',
  'error',
];

/** Insert cadence from the reference video: chunks arrive, all concurrent, later ones start later. */
const INSERT_DEMO_CHUNKS = [
  '帮我',
  '帮我查找一',
  '帮我查找一下',
  '帮我查找一下10',
  '帮我查找一下10六号',
];
const INSERT_DEMO_GAPS_MS = [420, 560, 640, 540, 280];

function getPreviewCapsulePayload() {
  if (isTauri || typeof window === 'undefined') {
    return {
      state: 'idle' as CapsuleState,
      level: 0,
      message: undefined,
      translation: false,
      warming: false,
      selectionPolish: false,
      style: 'siri' as CapsuleStyle,
      insertDemo: false,
    };
  }

  const params = new URLSearchParams(window.location.search);
  const stateParam = params.get('state');
  const previewState = CAPSULE_PREVIEW_STATES.includes(stateParam as CapsuleState)
    ? (stateParam as CapsuleState)
    : 'recording';
  const previewLevel = Number(params.get('level') ?? 0.6);
  const insertDemo = params.get('insertDemo') === '1';
  return {
    state: insertDemo ? ('recording' as CapsuleState) : previewState,
    level: Number.isFinite(previewLevel) ? Math.min(1, Math.max(0, previewLevel)) : 0.6,
    message: params.get('message') ?? undefined,
    translation: params.get('translation') === '1',
    warming: params.get('warming') === '1',
    selectionPolish: params.get('selectionPolish') === '1',
    style: parseCapsuleStyle(params.get('style')) ?? 'siri',
    insertDemo,
  };
}

interface CapsuleProps {
  os?: OS | null;
}

export function Capsule({ os: forcedOs }: CapsuleProps = {}) {
  const { t } = useTranslation();
  const os = forcedOs ?? detectOS();
  const preview = useMemo(() => getPreviewCapsulePayload(), []);
  const metrics = getCapsulePillMetrics(os);
  const [state, setState] = useState<CapsuleState>(preview.state);
  const [level, setLevel] = useState<number>(preview.level);
  const [message, setMessage] = useState<string | undefined>(preview.message);
  const [localAsrText, setLocalAsrText] = useState('');
  const [transcriptEnabled, setTranscriptEnabled] = useState(
    () => isTauri || new URLSearchParams(window.location.search).get('transcript') !== '0',
  );
  const [transcriptFontSize, setTranscriptFontSize] = useState(() =>
    capsuleTranscriptFontSize(
      isTauri
        ? undefined
        : Number(new URLSearchParams(window.location.search).get('fontSize') || 14),
    ),
  );
  const transcriptViewRef = useRef<TranscriptViewState>(createTranscriptViewState());
  const capsuleStateRef = useRef<CapsuleState>(preview.state);
  const [translation, setTranslation] = useState<boolean>(preview.translation);
  const [selectionPolish, setSelectionPolish] = useState<boolean>(preview.selectionPolish);
  // Preference events restyle live; recording state carries the same style so the
  // first show also renders correctly.
  const [capsuleStyle, setCapsuleStyle] = useState<CapsuleStyle>(preview.style);
  const stylePreferenceReadyRef = useRef(false);
  const isClassic = capsuleStyle === 'classic';
  const isTypeless = capsuleStyle === 'typeless';
  // Windows keeps the transcript in the dedicated capsule-rail HWND. The browser
  // preview and other platforms continue rendering the sibling rail in this tree.
  const splitTranscriptRail = isTauri && os === 'win';
  // Warming: the mic hasn't produced its first PCM frame yet. When true, the orb runs
  // its standby breathing form (see SiriGL warming).
  const [warming, setWarming] = useState<boolean>(preview.warming);
  // Moving average of warm→ready duration, driving the predicted expand pace of the
  // orb (see SiriGL warmProgress).
  const [warmupMs, setWarmupMs] = useState<number>(() => readWarmupMs());
  const warmStartRef = useRef<number | null>(null);
  // Classic pill only: done-state "inserted N chars" and thinking/using copy. The
  // payload only arrives on state change, so a ref suffices — setState triggers the
  // re-render; the ref just holds the final value for rendering.
  const insertedCharsRef = useRef(0);
  const operatingRef = useRef(false);
  // `leaving` + `lastVisibleState` implement the exit animation:
  // - On non-idle → idle, don't unmount immediately; set leaving=true and keep the last
  //   visible state (lastVisibleState) so the capsule shrinks/fades via capsule-out.
  // - After the animation (EXIT_ANIM_MS), set leaving=false to return to the truly
  //   unmounted branch.
  // - If state flips back to non-idle meanwhile (e.g. hotkey pressed again), abort
  //   leaving immediately and restore display.
  const [leaving, setLeaving] = useState<boolean>(false);
  const [lastVisibleState, setLastVisibleState] = useState<CapsuleState>(preview.state);
  const [lastVisibleSelectionPolish, setLastVisibleSelectionPolish] = useState<boolean>(
    preview.selectionPolish,
  );
  const [lastVisibleMessage, setLastVisibleMessage] = useState<string | undefined>(preview.message);
  // Exit duration follows the style; the leaving effect deliberately depends only on
  // state, so read the latest value through the ref.
  const exitMsRef = useRef(capsuleStyle === 'siri' ? EXIT_ANIM_MS_SIRI : EXIT_ANIM_MS_CLASSIC);
  exitMsRef.current = capsuleStyle === 'siri' ? EXIT_ANIM_MS_SIRI : EXIT_ANIM_MS_CLASSIC;
  const exitMs = exitMsRef.current;
  // Vocab suggestion card. Uses its own event channel, not the session state machine —
  // that machine carries Esc capture, Space attachment, multi-monitor positioning, and
  // a non-session state would only pollute it.
  const [suggestions, setSuggestions] = useState<PendingCorrection[]>([]);
  // Insert-failure fallback card. Same pattern as the vocab card: own event channel,
  // not part of the session state machine.
  const [insertFallback, setInsertFallback] = useState<InsertFallbackCardPayload | null>(null);
  const hostMetrics = getCapsuleHostMetrics(os, translation, capsuleStyle);
  const badgeBottom = Math.round(metrics.height * 0.73);

  // Warm up the shader compile cache while idle so the first hotkey press doesn't
  // compile on the spot (perceived response latency).
  useEffect(() => {
    const idle = (window as Window & { requestIdleCallback?: (cb: () => void) => number })
      .requestIdleCallback;
    if (idle) {
      idle(() => warmUpSiriShaders());
      return;
    }
    const timer = setTimeout(() => warmUpSiriShaders(), 1200);
    return () => clearTimeout(timer);
  }, []);

  useEffect(() => {
    if (!isTauri) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    (async () => {
      const { listen } = await import('@tauri-apps/api/event');
      const handle = await listen<CapsulePayload>('capsule:state', (event) => {
        const p = event.payload;
        if (!capsuleStateFirstLogged) {
          capsuleStateFirstLogged = true;
          // #470 diagnostics v2: confirm the capsule webview really received the backend
          // event — distinguishes "backend never emitted" from "emitted but the window
          // didn't show/render". Pair with backend [capsule] logs to locate the cause.
          console.info('[capsule] first capsule:state received in webview, state=', p.state);
        }
        setState(p.state);
        setLevel(p.level ?? 0);
        setMessage(p.message ?? undefined);
        const previous = capsuleStateRef.current;
        capsuleStateRef.current = p.state;
        // Any new recording session must drop prior ASR text — including when
        // the previous capsule was still transcribing/polishing/done from
        // dictation (selection-voice Start used to keep the stale stream).
        if (p.state === 'recording' && previous !== 'recording') {
          transcriptViewRef.current = beginTranscriptGeneration(transcriptViewRef.current);
          setLocalAsrText('');
        }
        if (
          p.state === 'idle' ||
          p.state === 'done' ||
          p.state === 'cancelled' ||
          p.state === 'error'
        ) {
          // Terminal payloads must release the native transcript rail before the exit/card
          // branch can leave the capsule webview. Keeping the old text here would make the
          // enlarged native window stay interactive after the session has ended.
          transcriptViewRef.current = clearTranscriptText(transcriptViewRef.current);
          setLocalAsrText('');
        }
        setTranslation(p.translation === true);
        setWarming(p.warming === true);
        setSelectionPolish(p.selectionPolish === true);
        const style = parseCapsuleStyle(p.capsuleStyle);
        if (style && !stylePreferenceReadyRef.current) setCapsuleStyle(style);
        if (p.insertedChars != null) insertedCharsRef.current = p.insertedChars;
        operatingRef.current = p.operating === true;
      });
      const transcriptHandle = await listen<BackendEvent>('backend:event', (event) => {
        const next = applyTranscriptEvent(transcriptViewRef.current, event.payload);
        transcriptViewRef.current = next;
        setLocalAsrText(next.text);
      });
      const suggestHandle = await listen<PendingCorrection[]>('vocab:suggested', (event) => {
        setSuggestions(event.payload ?? []);
      });
      const fallbackHandle = await listen<InsertFallbackCardPayload | null>(
        'insert:fallback',
        (event) => {
          setInsertFallback(event.payload ?? null);
        },
      );
      if (cancelled) {
        handle();
        transcriptHandle();
        suggestHandle();
        fallbackHandle();
      } else {
        unlisten = () => {
          handle();
          transcriptHandle();
          suggestHandle();
          fallbackHandle();
        };
      }
    })();
    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, []);

  // Subscribe before reading preferences, covering both the first load of a hidden
  // window and a live restyle mid-session.
  useEffect(() => {
    if (!isTauri) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    let preferenceRevision = 0;
    (async () => {
      const { listen } = await import('@tauri-apps/api/event');
      const handle = await listen<UserPreferences>('prefs:changed', (event) => {
        setTranscriptEnabled(event.payload.capsuleTranscriptEnabled ?? true);
        setTranscriptFontSize(capsuleTranscriptFontSize(event.payload.capsuleTranscriptFontSize));
        preferenceRevision += 1;
        const next = parseCapsuleStyle(event.payload?.capsuleStyle);
        if (next) {
          stylePreferenceReadyRef.current = true;
          setCapsuleStyle(next);
        }
      });
      if (cancelled) {
        handle();
        return;
      }
      unlisten = handle;
      const revisionAtRead = preferenceRevision;
      const preferences = await getSettings();
      const next = parseCapsuleStyle(preferences.capsuleStyle);
      if (!cancelled && revisionAtRead === preferenceRevision && next) {
        setTranscriptEnabled(preferences.capsuleTranscriptEnabled ?? true);
        setTranscriptFontSize(capsuleTranscriptFontSize(preferences.capsuleTranscriptFontSize));
        stylePreferenceReadyRef.current = true;
        setCapsuleStyle(next);
      }
    })().catch((error) => console.warn('[capsule] preferences subscription failed', error));
    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, []);

  // Exit animation schedule: when state truly enters idle, play capsule-out for
  // EXIT_ANIM_MS_SIRI / EXIT_ANIM_MS_CLASSIC (per current style, read via exitMsRef),
  // then unmount. Design notes:
  // 1. Entering non-idle: clear leaving, record the latest visible state;
  // 2. Entering idle while previously visible: set leaving and start the timer;
  // 3. Flipped back to non-idle meanwhile: cleanup clearTimeout, the timer never fires,
  //    and the next effect run restores the visible state — never switching a visible
  //    state to idle by mistake.
  useEffect(() => {
    if (state !== 'idle') {
      // Restore visibility immediately and cancel any pending exit from the last round.
      if (leaving) setLeaving(false);
      setLastVisibleState(state);
      return undefined;
    }
    // state === 'idle': only proceed when transitioning from a visible state.
    if (lastVisibleState === 'idle') return undefined;
    setLeaving(true);
    const timer = setTimeout(() => {
      setLeaving(false);
      setLastVisibleState('idle');
    }, exitMsRef.current);
    return () => clearTimeout(timer);
    // Deliberately depends only on state — lastVisibleState / leaving are internal
    // derived values; adding them to deps would rebuild the timer repeatedly.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state]);

  // Idle payloads carry no final-state copy. Keeping the last visible selection notice
  // lets the exit animation after auto-hide continue showing "replaced / no selection"
  // feedback instead of flashing back to the voice stage.
  useEffect(() => {
    if (state === 'idle') return;
    setLastVisibleSelectionPolish(selectionPolish);
    setLastVisibleMessage(message);
  }, [state, selectionPolish, message]);

  // Learn how long the warm phase lasts (= mic-ready time); EMA-update warmupMs, the
  // baseline for the predicted expand duration.
  useEffect(() => {
    if (warming) {
      warmStartRef.current = performance.now();
      return;
    }
    if (warmStartRef.current == null) return;
    const loadMs = performance.now() - warmStartRef.current;
    warmStartRef.current = null;
    // Filter outliers: <20ms is likely not a real entry; >3s is likely first-run TCC or
    // a stall, not the norm.
    if (loadMs < 20 || loadMs > 3000) return;
    setWarmupMs((prev) => {
      const next = Math.min(600, Math.max(60, prev * 0.7 + loadMs * 0.3));
      try {
        localStorage.setItem(WARMUP_MS_KEY, String(Math.round(next)));
      } catch {
        /* ignore */
      }
      return next;
    });
  }, [warming]);

  useEffect(() => {
    if (!preview.insertDemo) return undefined;
    let cancelled = false;
    let elapsed = 0;
    const timers: number[] = [];
    INSERT_DEMO_CHUNKS.forEach((chunk, index) => {
      elapsed += INSERT_DEMO_GAPS_MS[index] ?? 480;
      timers.push(
        window.setTimeout(() => {
          if (!cancelled) setLocalAsrText(chunk);
        }, elapsed),
      );
    });
    return () => {
      cancelled = true;
      for (const timer of timers) window.clearTimeout(timer);
    };
  }, [preview.insertDemo]);

  // Derive these values before any conditional return. Hooks must be called for the same
  // component shape while the capsule switches between the live pill, fallback card,
  // suggestion card, and the hidden idle branch.
  const renderedState: CapsuleState = state === 'idle' ? lastVisibleState : state;
  const renderedSelectionPolish = state === 'idle' ? lastVisibleSelectionPolish : selectionPolish;
  const renderedMessage =
    state === 'idle'
      ? lastVisibleMessage
      : state === 'transcribing' && localAsrText
        ? localAsrText
        : message;
  const liveTranscript = visibleCapsuleTranscript(
    localAsrText,
    transcriptEnabled,
    renderedState,
    renderedSelectionPolish,
  );
  const showLiveTranscript = state !== 'idle' && liveTranscript.length > 0;
  const transcriptVisible = showLiveTranscript && !insertFallback && suggestions.length === 0;

  useEffect(() => {
    if (!isTauri) return;
    void setCapsuleTranscriptVisible(transcriptVisible).catch((error) =>
      console.warn('[capsule] transcript visibility sync failed', error),
    );
  }, [transcriptVisible]);

  // Always release the native hit-test region when the capsule webview is torn down.
  useEffect(() => {
    if (!isTauri) return;
    return () => {
      void setCapsuleTranscriptVisible(false).catch((error) =>
        console.warn('[capsule] transcript visibility cleanup failed', error),
      );
    };
  }, []);

  // Fallback card comes first: it pops the moment the session ends, while the capsule
  // is still rendering the Done/Error terminal state — and the session's outcome was
  // exactly "didn't insert", so letting the terminal state cover it would report a
  // false success. The backend yields in sync: while the card is visible, idle hides
  // without closing the window (see capsule_focus.rs).
  if (insertFallback) {
    return <InsertFallbackCard payload={insertFallback} />;
  }

  // Card takes priority: it shares the window with the capsule but the timing doesn't
  // conflict — it pops after the text is corrected, when the session has long ended;
  // the backend dismisses it when a new dictation starts (see begin_session_as).
  if (suggestions.length > 0) {
    return <VocabSuggestionCard suggestions={suggestions} />;
  }

  // Truly unmounted: state is idle and no exit animation is running.
  if (state === 'idle' && !leaving) {
    return <div style={{ width: 0, height: 0 }} />;
  }

  return (
    <div
      style={{
        width: '100%',
        height: '100%',
        position: 'relative',
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'flex-end',
        paddingLeft: hostMetrics.horizontalInset,
        paddingRight: hostMetrics.horizontalInset,
        paddingBottom: hostMetrics.bottomInset,
        boxSizing: hostMetrics.boxSizing,
        background: preview.insertDemo
          ? 'radial-gradient(ellipse at 50% 35%, #4d9a4a 0%, #1a4a22 42%, #0e2414 100%)'
          : 'transparent',
        animation: leaving
          ? `capsule-out ${exitMs}ms cubic-bezier(.55,.06,.68,.19) forwards`
          : // The .68s entry got "delay after keypress" feedback: cut to .38s with the
            // curve keeping a slight bounce, so the orb appears almost immediately.
            'capsule-in .38s cubic-bezier(.3,1.2,.4,1) both',
        transformOrigin: 'center',
        willChange: 'transform, opacity',
      }}
    >
      {!renderedSelectionPolish &&
        (isClassic ? (
          <ClassicCapsule
            os={os}
            state={renderedState}
            level={leaving ? 0 : level}
            insertedChars={insertedCharsRef.current}
            message={renderedMessage}
            transcript={splitTranscriptRail ? '' : liveTranscript}
            transcriptFontSize={transcriptFontSize}
            transcriptVisible={transcriptVisible}
            transcriptInSameWindow={!splitTranscriptRail}
            operating={operatingRef.current}
            translation={translation}
          />
        ) : isTypeless ? (
          <TypelessCapsule
            state={renderedState}
            level={leaving ? 0 : level}
            insertedChars={insertedCharsRef.current}
            message={renderedMessage}
            transcript={splitTranscriptRail ? '' : liveTranscript}
            transcriptFontSize={transcriptFontSize}
            operating={operatingRef.current}
            translation={translation}
            translationInSameWindow={!splitTranscriptRail}
            warming={!leaving && warming}
          />
        ) : (
          <>
            {/* "Translating" badge — two nested layers:
          the outer layer only does absolute positioning + horizontal centering
          (translateX(-50%)) with no animation; the inner layer only does vertical
          shift + fade. This avoids conflicting with translateX(-50%) and avoids
          keyframe vs inline transform overrides causing visual jumps. */}
            <div
              style={{
                position: 'absolute',
                left: '50%',
                bottom: `${badgeBottom}px`,
                transform: 'translateX(-50%)',
                pointerEvents: 'none',
              }}
            >
              <div
                style={{
                  display: 'inline-flex',
                  alignItems: 'center',
                  gap: 5,
                  fontSize: 10.5,
                  fontWeight: 600,
                  color: 'var(--ol-capsule-center-ink)',
                  background: 'var(--ol-capsule-badge-bg)',
                  border: '1px solid var(--ol-capsule-pill-border)',
                  borderRadius: 999,
                  padding: '4px 10px',
                  letterSpacing: '0.02em',
                  whiteSpace: 'nowrap',
                  // Hidden: starts just below the orb; shown: settles above the orb.
                  opacity: translation ? 1 : 0,
                  transform: translation ? 'translateY(0) scale(1)' : 'translateY(40px) scale(.88)',
                  transformOrigin: 'center bottom',
                  transition: 'opacity .24s ease-out, transform .34s cubic-bezier(.2,.9,.3,1.1)',
                  willChange: 'opacity, transform',
                }}
              >
                <span
                  style={{
                    width: 5,
                    height: 5,
                    borderRadius: 999,
                    background: 'rgba(150, 185, 255, 0.95)',
                    boxShadow: '0 0 8px rgba(90, 140, 255, 0.9)',
                  }}
                />
                {t('capsule.translating')}
              </div>
            </div>
            <div className="ol-siri-capsule-stack">
              {showLiveTranscript && !splitTranscriptRail && (
                <CapsuleTranscriptRail
                  text={liveTranscript}
                  fontSize={transcriptFontSize}
                  tone="frost"
                  stageWidth={hostMetrics.width}
                  maxWidth={hostMetrics.width - 24}
                  height={getCapsuleTranscriptRailHeight('siri')}
                />
              )}
              <div
                style={{
                  opacity: 1,
                  transition: 'opacity .28s var(--ol-motion-soft)',
                }}
              >
                <VoiceOrbStage
                  os={os}
                  state={renderedState}
                  level={leaving ? 0 : level}
                  warming={!leaving && warming}
                  warmupMs={warmupMs}
                  message={renderedMessage}
                />
              </div>
            </div>
          </>
        ))}
      {renderedSelectionPolish && (
        <SelectionPolishNotice state={renderedState} message={renderedMessage} />
      )}
    </div>
  );
}
