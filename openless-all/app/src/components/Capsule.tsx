import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { detectOS, type OS } from './WindowChrome';
import { warmUpSiriShaders } from './SiriGL';
import { useReducedMotion } from '../lib/motion';
import { VoiceOrbStage } from './VoiceOrbStage';
import { DEFAULT_CAPSULE_EXIT_MS, OpenLessPill } from './OpenLessCapsule';
import { CapsuleTranscriptRail, CapsuleTranslationBadge } from './LiveTranscriptPill';
import {
  capsuleTranscriptBandPresent,
  capsuleTranscriptFontSize,
  visibleCapsuleTranscript,
} from '../lib/capsuleTranscript';
import { getSettings } from '../lib/ipc/settings';
import { cancelDictation, setCapsuleTranscriptVisible, stopDictation } from '../lib/ipc/dictation';
import {
  getCapsuleHostMetrics,
  getCapsulePillMetrics,
  getClassicCapsuleGeometry,
  getCapsuleTranscriptRailHeight,
  parseCapsuleStyle,
} from '../lib/capsuleLayout';
import { getCapsuleSnapshot, isTauri } from '../lib/ipc';
import { SIRI_ENHANCED_GLYPH_MOTION } from '../lib/insertTextAnimation';
import { useTranscriptLinger } from '../lib/useTranscriptLinger';
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
  applyTranscriptSessionId,
  applyTranscriptSnapshot,
  beginTranscriptGeneration,
  clearTranscriptText,
  createTranscriptViewState,
  shouldRetryTranscriptSnapshot,
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
        fontWeight: 'var(--ol-font-weight-semibold)',
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
// Preserve the native Classic host/rail geometry; OpenLessPill owns the new
// white-and-blue recording/processing surface inside that stable host.

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
  leaving: boolean;
  warming: boolean;
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
  leaving,
  warming,
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
      {/* The language picker belongs to the interactive capsule, even when
          Windows puts the transcript in a separate click-through rail. */}
      <div
        style={{
          position: 'absolute',
          left: '50%',
          bottom: classicGeometry.badgeBottomOffset,
          transform: 'translateX(-50%)',
          pointerEvents: 'none',
        }}
      >
        <CapsuleTranslationBadge visible={translation && state === 'recording' && !leaving} />
      </div>
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
        <OpenLessPill
          os={os}
          state={state}
          level={level}
          insertedChars={insertedChars}
          message={message}
          operating={operating}
          translation={translation}
          leaving={leaving}
          warming={warming}
          onCancel={onCancel}
          onConfirm={onConfirm}
        />
      </div>
    </>
  );
}

// Must stay in sync with the @keyframes capsule-out duration — otherwise the timer
// unmounts before the animation ends and the user sees it cut off mid-flight. Both
// The spectral renderer exits in 520ms; the default capsule contracts in 180ms.
const EXIT_ANIM_MS_SIRI = 520;
const EXIT_ANIM_MS_CLASSIC = DEFAULT_CAPSULE_EXIT_MS;

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
      style: 'classic' as CapsuleStyle,
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
    style: parseCapsuleStyle(params.get('style')) ?? 'classic',
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
  const reducedMotion = useReducedMotion();
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
  const [siriEnhanced, setSiriEnhanced] = useState(false);
  const transcriptViewRef = useRef<TranscriptViewState>(createTranscriptViewState());
  const capsuleStateRef = useRef<CapsuleState>(preview.state);
  const [translation, setTranslation] = useState<boolean>(preview.translation);
  const [selectionPolish, setSelectionPolish] = useState<boolean>(preview.selectionPolish);
  // Preference events restyle live; recording state carries the same style so the
  // first show also renders correctly.
  const [capsuleStyle, setCapsuleStyle] = useState<CapsuleStyle>(preview.style);
  const stylePreferenceReadyRef = useRef(false);
  const isClassic = capsuleStyle === 'classic';
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
    const unlisteners: (() => void)[] = [];
    let cancelled = false;
    let stateRevision = 0;
    let suggestionsRevision = 0;
    let fallbackRevision = 0;
    const cleanupListeners = () => {
      for (const unlisten of unlisteners.splice(0)) unlisten();
    };
    (async () => {
      const { listen } = await import('@tauri-apps/api/event');
      const { getCurrentWebviewWindow } = await import('@tauri-apps/api/webviewWindow');
      const target = { kind: 'WebviewWindow' as const, label: getCurrentWebviewWindow().label };
      if (cancelled) return;
      const subscribe = async <T,>(name: string, apply: (payload: T) => void) => {
        // Tauri's default Any listener also receives emit_to(otherWindow). In particular,
        // hiding a mirror sends it idle; that must never exit the primary capsule.
        const unlisten = await listen<T>(
          name,
          (event) => {
            if (!cancelled) apply(event.payload);
          },
          name === 'backend:event' ? undefined : { target },
        );
        if (cancelled) {
          unlisten();
          return false;
        }
        unlisteners.push(unlisten);
        return true;
      };
      const replayActiveSessionTranscript = async (sessionId: string) => {
        const snapshot = await getCapsuleSnapshot().catch((error) => {
          if (!cancelled) console.warn('[capsule] active session snapshot replay failed', error);
          return undefined;
        });
        if (cancelled || !snapshot || snapshot.sessionId !== sessionId) return;
        const current = transcriptViewRef.current;
        if (shouldRetryTranscriptSnapshot(current, snapshot)) return;
        const next = applyTranscriptSnapshot(current, snapshot);
        if (next === current) return;
        transcriptViewRef.current = next;
        setLocalAsrText(next.text);
      };
      const applyPayload = (p: CapsulePayload) => {
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
        const previousTranscript = transcriptViewRef.current;
        let nextTranscript = applyTranscriptSessionId(previousTranscript, p.sessionId);
        const sessionChanged = nextTranscript !== previousTranscript;
        // Any new recording session must drop prior ASR text — including when
        // the previous capsule was still transcribing/polishing/done from
        // dictation (selection-voice Start used to keep the stale stream).
        const sessionId = p.sessionId;
        const reactivatedSession =
          p.state === 'recording' &&
          previous !== 'recording' &&
          !sessionChanged &&
          !previousTranscript.sessionStartObserved &&
          sessionId != null &&
          nextTranscript.sessionId === sessionId;
        if (reactivatedSession) {
          void replayActiveSessionTranscript(sessionId);
        } else if (p.state === 'recording' && previous !== 'recording' && !sessionChanged) {
          nextTranscript = beginTranscriptGeneration(nextTranscript);
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
          nextTranscript = clearTranscriptText(nextTranscript);
        }
        transcriptViewRef.current = nextTranscript;
        if (nextTranscript !== previousTranscript) setLocalAsrText(nextTranscript.text);
        setTranslation(p.translation === true);
        setWarming(p.warming === true);
        setSelectionPolish(p.selectionPolish === true);
        const style = parseCapsuleStyle(p.capsuleStyle);
        if (style && !stylePreferenceReadyRef.current) setCapsuleStyle(style);
        if (p.insertedChars != null) insertedCharsRef.current = p.insertedChars;
        operatingRef.current = p.operating === true;
      };
      if (
        !(await subscribe<CapsulePayload>('capsule:state', (payload) => {
          stateRevision += 1;
          applyPayload(payload);
        }))
      )
        return;
      if (
        !(await subscribe<BackendEvent>('backend:event', (payload) => {
          const next = applyTranscriptEvent(transcriptViewRef.current, payload);
          transcriptViewRef.current = next;
          setLocalAsrText(next.text);
        }))
      )
        return;
      if (
        !(await subscribe<PendingCorrection[]>('vocab:suggested', (payload) => {
          suggestionsRevision += 1;
          setSuggestions(payload ?? []);
        }))
      )
        return;
      if (
        !(await subscribe<InsertFallbackCardPayload | null>('insert:fallback', (payload) => {
          fallbackRevision += 1;
          setInsertFallback(payload ?? null);
        }))
      )
        return;

      // Disposable cards can miss their first event while the WebView is mounting.
      const suggestionsAtRead = suggestionsRevision;
      const fallbackAtRead = fallbackRevision;
      try {
        const { invoke } = await import('@tauri-apps/api/core');
        if (cancelled) return;
        const snapshot = await invoke<{
          suggestions: PendingCorrection[];
          fallback: InsertFallbackCardPayload | null;
        }>('get_capsule_auxiliary_snapshot');
        if (cancelled) return;
        if (suggestionsRevision === suggestionsAtRead) setSuggestions(snapshot.suggestions);
        if (fallbackRevision === fallbackAtRead) setInsertFallback(snapshot.fallback);
      } catch {
        // Mobile hosts do not expose desktop card snapshots.
      }

      // Lazy mirrors can miss the session start. Subscribe first, then replay a coherent
      // frame only if no newer state arrived while the native request was in flight.
      for (const delayMs of [0, 16, 32, 64, 128, 256]) {
        if (cancelled) return;
        if (delayMs > 0) {
          await new Promise<void>((resolve) => window.setTimeout(resolve, delayMs));
        }
        if (cancelled) return;
        const revisionAtRead = stateRevision;
        const snapshot = await getCapsuleSnapshot().catch((error) => {
          if (!cancelled) console.warn('[capsule] snapshot replay failed', error);
          return undefined;
        });
        if (cancelled || snapshot === undefined) return;
        if (!snapshot || stateRevision !== revisionAtRead) continue;
        const previousTranscript = transcriptViewRef.current;
        if (shouldRetryTranscriptSnapshot(previousTranscript, snapshot)) continue;
        const nextTranscript = applyTranscriptSnapshot(previousTranscript, snapshot);
        if (nextTranscript === previousTranscript) return;
        transcriptViewRef.current = nextTranscript;
        setLocalAsrText(nextTranscript.text);
        applyPayload(snapshot);
        return;
      }
    })().catch((error) => {
      cleanupListeners();
      if (!cancelled) console.warn('[capsule] subscription failed', error);
    });
    return () => {
      cancelled = true;
      cleanupListeners();
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
        setSiriEnhanced(event.payload.capsuleSiriEnhanced ?? false);
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
        setSiriEnhanced(preferences.capsuleSiriEnhanced ?? false);
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
  // Siri enhanced shows the rail from the start of recording, before any words, and keeps it up
  // briefly after the session for its collapse (CapsuleTranscriptOverlay plays both).
  const enhancedRail = splitTranscriptRail && capsuleStyle === 'siri' && siriEnhanced;
  const railLinger = useTranscriptLinger(
    liveTranscript,
    enhancedRail
      ? capsuleTranscriptBandPresent(transcriptEnabled, state, selectionPolish, warming)
      : liveTranscript.length > 0,
    enhancedRail,
    SIRI_ENHANCED_GLYPH_MOTION.closeMs,
  );
  const showLiveTranscript = state !== 'idle' && railLinger.visible;
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
  if (state === 'idle' && lastVisibleState === 'idle' && !leaving) {
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
        animation: reducedMotion
          ? 'none'
          : isClassic
            ? 'none'
            : leaving
              ? `capsule-out ${exitMs}ms cubic-bezier(.55,.06,.68,.19) forwards`
              : // The .68s entry got "delay after keypress" feedback: cut to .38s with the
                // curve keeping a slight bounce, so the orb appears almost immediately.
                'capsule-in .38s cubic-bezier(.3,1.2,.4,1) both',
        transformOrigin: 'center',
        willChange: isClassic ? 'auto' : 'transform, opacity',
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
            leaving={leaving}
            warming={warming}
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
                  fontWeight: 'var(--ol-font-weight-semibold)',
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
