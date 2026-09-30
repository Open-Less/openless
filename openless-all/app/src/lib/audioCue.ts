// Record start cue: synthesizes a short rising two-tone with the Web Audio API on the fly;
// no audio files are bundled.
// Two operations:
//   - playRecordStartCue()  play (called when the record hotkey is pressed and recording starts)
//   - stopAudioCue()        close/stop (called on leaving recording, or on rapid hotkey presses
//                           to avoid overlapping audio)
//
// Triggered in the capsule window (always alive, receives capsule:state events); the settings
// page "preview" reuses the same code.
// Design principle: in any environment (no Web Audio, AudioContext suspended by autoplay
// policy, per-tone creation failure) silently degrade; never throw into the recording flow.

/** Synthesis parameters for one sine tone (relative to the cue start). */
export interface CueTone {
  /** Frequency (Hz). */
  freq: number;
  /** Start time relative to the cue start (ms). */
  startMs: number;
  /** Duration (ms). */
  durationMs: number;
  /** Peak gain of the exponential envelope (0..1); controls loudness. */
  peakGain: number;
}

// Rising minor third two-tone (A5 880Hz -> C#6 1108.73Hz): a clear, light, non-harsh sound for
// "recording started". The tones overlap slightly so they read as one "ding-dong" rather than
// two separate beeps. Pure data -> easy to unit test.
export function recordStartCueTones(): CueTone[] {
  return [
    { freq: 880, startMs: 0, durationMs: 130, peakGain: 0.16 },
    { freq: 1108.73, startMs: 95, durationMs: 170, peakGain: 0.18 },
  ];
}

/** Total cue duration (ms) = end time of the last tone; callers use it to schedule stop / preview feedback. */
export function cueTotalDurationMs(tones: CueTone[]): number {
  return tones.reduce((max, t) => Math.max(max, t.startMs + t.durationMs), 0);
}

// Safari/WKWebView legacy prefix; structured type (not any) to reach the webkit fallback ctor.
type AudioContextCtor = typeof AudioContext;
interface WebkitWindow {
  webkitAudioContext?: AudioContextCtor;
}

// Module-level singleton. Each Tauri window is a separate webview = separate JS module
// instance, so the capsule window and settings window each hold their own ctx / activeVoices
// and do not interfere.
let sharedCtx: AudioContext | null = null;
interface ActiveVoice {
  osc: OscillatorNode;
  gain: GainNode;
}
let activeVoices: ActiveVoice[] = [];
// A suspended AudioContext must be resumed (async) before scheduling. Two kinds of events can
// happen while waiting for resume; two sequence numbers track them separately, avoiding both
// extremes — "quick recording loses the cue entirely" and "cue arrives after recording stopped":
//   playSeq — incremented per new play request; if a newer round took over when resume settles,
//             this one yields (prevents overlap).
//   stopSeq — incremented per stopAudioCue; if a stop happened while waiting for resume, decide
//             by "really late or not".
let playSeq = 0;
let stopSeq = 0;

// When recording already ended during resume (a stop occurred), the cue counts as really late
// (and is dropped) only if request -> resume took longer than this threshold; within the
// threshold the cue still plays — a quick tap of record (ended before resume finished) still
// gets feedback.
const DEFERRED_CUE_LATE_THRESHOLD_MS = 400;
const RESUME_TIMEOUT_MS = 250;
let recoveryTimer: ReturnType<typeof setTimeout> | undefined;

function clearRecoveryTimer(): void {
  clearTimeout(recoveryTimer);
  recoveryTimer = undefined;
}

// Without performance (theoretical fallback; always present in Tauri WebView) return 0:
// elapsedMs is always 0 and never counts as late — better to replay one cue than lose it,
// the safe direction consistent with the "fix lost cue" goal.
function nowMs(): number {
  return typeof performance !== 'undefined' ? performance.now() : 0;
}

/**
 * After resume completes, decide whether a cue scheduled during the suspension should still
 * play. Pure function, easy to unit test:
 * - Superseded by a newer play round -> do not play (the new round owns it; avoids overlap).
 * - Recording stopped while waiting and it is really late (over threshold) -> do not play
 *   (avoids a late cue).
 * - Otherwise (including "stopped but resume was fast" quick recordings) -> play anyway.
 */
export function shouldPlayDeferredCue(params: {
  superseded: boolean;
  stoppedWhileWaiting: boolean;
  elapsedMs: number;
  lateThresholdMs: number;
}): boolean {
  if (params.superseded) return false;
  if (params.stoppedWhileWaiting && params.elapsedMs > params.lateThresholdMs) return false;
  return true;
}

/** What to do with the held AudioContext before playing the cue. */
export type AudioContextAction = 'ready' | 'resume' | 'recreate';

/**
 * Decide how to handle the AudioContext based on its runtime state. Pure function, easy to
 * unit test; core of the "no sound after long use" bug fix:
 *  - 'closed'  -> 'recreate': a closed ctx cannot be revived (resume() rejects,
 *                createOscillator() throws), it must be discarded and rebuilt. Over long use,
 *                frequent recording seizing the audio session can get the shared
 *                WKWebView/WebView2 ctx closed by the system and never rebuilt — the start cue
 *                and settings preview both go permanently silent (each window holds its own
 *                ctx, both degrade this way).
 *  - 'running' -> 'ready': synthesis can be scheduled directly.
 *  - Anything else ('suspended' / WebKit non-standard 'interrupted' / any unknown non-running
 *                state) -> 'resume': wake it with an async resume first, then schedule.
 */
export function audioContextActionForState(state: string): AudioContextAction {
  if (state === 'closed') return 'recreate';
  if (state === 'running') return 'ready';
  return 'resume';
}

/** What to do with the cue after resume() completes. */
export type CueAction = 'schedule' | 'recreate-retry' | 'drop';

/**
 * How to dispose of the cue after resume() finishes. Pure function, easy to unit test; the
 * decision core of the "no sound after long use" fix.
 *
 * Key fix: over long use, repeated local ASR / recording sessions pushing the shared WKWebView
 * AudioContext into 'suspended' or WebKit's non-standard 'interrupted' make resume() get
 * rejected, or nominally resolve while the ctx is still not 'running' (currentTime frozen). The
 * old code either swallowed resume failures silently or scheduled on a frozen clock — both
 * routes silence the cue permanently until process restart. Both "unwakeable" degraded states
 * are treated the same here: discard the dead ctx and retry once with a fresh one, letting the
 * shared ctx self-heal. ('closed' is already recreated in getContext and never reaches here.)
 *  - Resume succeeded and the ctx really runs: play if still due -> 'schedule', else 'drop'
 *    (superseded / really late).
 *  - Otherwise (rejected / resolved but not running): if rebuild is allowed -> 'recreate-retry';
 *    if not (already retried once) -> 'drop', avoiding infinite recursion on a dead ctx.
 */
export function cueActionAfterResume(params: {
  runningAfterResume: boolean;
  shouldPlay: boolean;
  allowRecreate: boolean;
}): CueAction {
  if (!params.shouldPlay) return 'drop';
  if (params.runningAfterResume) return 'schedule';
  return params.allowRecreate ? 'recreate-retry' : 'drop';
}

function resolveAudioContextCtor(): AudioContextCtor | null {
  if (typeof window === 'undefined') return null;
  // window.AudioContext comes from the global declaration; fetch the webkit-prefixed one via
  // a structured type to avoid any.
  const webkit = window as WebkitWindow;
  return window.AudioContext ?? webkit.webkitAudioContext ?? null;
}

function getContext(): AudioContext | null {
  const Ctor = resolveAudioContextCtor();
  if (!Ctor) return null;
  // A closed ctx cannot be revived; discard and rebuild — otherwise every later cue stays
  // silent forever. Also clear activeVoices attached to the old ctx so de-overlap never
  // touches stale nodes.
  if (sharedCtx && audioContextActionForState(sharedCtx.state) === 'recreate') {
    sharedCtx = null;
    activeVoices = [];
  }
  if (!sharedCtx) {
    try {
      sharedCtx = new Ctor();
    } catch {
      sharedCtx = null;
      return null;
    }
  }
  return sharedCtx;
}

// Discard an unwakeable / dead AudioContext: clear the module singleton (the next getContext
// rebuilds a fresh one) and its attached activeVoices, and best-effort close to release the
// underlying audio resources. Ignore if already closed or close fails.
function discardContext(ctx: AudioContext): void {
  if (sharedCtx === ctx) {
    stopVoices();
    sharedCtx = null;
  }
  try {
    void ctx.close().catch(() => undefined);
  } catch {
    // Already closed, or the implementation returned no Promise; ignore.
  }
}

// Stop currently sounding nodes (does not touch playSeq / stopSeq — de-overlap / cleanup only).
function stopVoices(): void {
  const ctx = sharedCtx;
  const now = ctx?.currentTime ?? 0;
  for (const { osc, gain } of activeVoices) {
    try {
      gain.gain.cancelScheduledValues(now);
      // An exponential ramp cannot reach 0; approximate silence with a tiny value, then stop
      // the oscillator immediately.
      gain.gain.setValueAtTime(0.0001, now);
      osc.stop(now + 0.02);
    } catch {
      // Already stopped / disconnected; ignore.
    }
  }
  activeVoices = [];
}

/** Close/stop the cue: stop playing nodes and mark "a stop happened meanwhile" so pending resume callbacks can judge lateness. */
export function stopAudioCue(): void {
  stopSeq++;
  stopVoices();
}

// Actually schedule the synthesis nodes. Must be called while the AudioContext is running
// (not suspended): when suspended, currentTime is frozen at the pause moment, so nodes would
// be scheduled at stale timestamps -> silent and piling up.
function scheduleCueVoices(ctx: AudioContext): void {
  // On rapid hotkey presses, stop the previous round first to keep overlap from building up.
  // Use stopVoices, not stopAudioCue: this must not invalidate its own generation.
  stopVoices();

  const base = ctx.currentTime + 0.01;
  for (const tone of recordStartCueTones()) {
    try {
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.type = 'sine';
      const t0 = base + tone.startMs / 1000;
      const tEnd = t0 + tone.durationMs / 1000;
      osc.frequency.setValueAtTime(tone.freq, t0);
      // 5ms attack + exponential release: avoids start/stop click pops.
      gain.gain.setValueAtTime(0.0001, t0);
      gain.gain.exponentialRampToValueAtTime(tone.peakGain, t0 + 0.005);
      gain.gain.exponentialRampToValueAtTime(0.0001, tEnd);
      osc.connect(gain).connect(ctx.destination);
      osc.start(t0);
      osc.stop(tEnd + 0.02);

      const voice: ActiveVoice = { osc, gain };
      activeVoices.push(voice);
      osc.onended = () => {
        activeVoices = activeVoices.filter((v) => v !== voice);
        try {
          osc.disconnect();
          gain.disconnect();
        } catch {
          // noop
        }
      };
    } catch {
      // One tone failing to create/schedule must not affect the rest.
    }
  }
}

/**
 * Prime the AudioContext: create and resume early so the ctx is already running when recording
 * actually starts and playRecordStartCue can schedule synchronously, bypassing the
 * suspended->resume async race — the root cause of lost cues on quick recordings.
 * Note: under a strict autoplay policy a resume without a user gesture may be rejected
 * (silently degraded), in which case priming does nothing and the playSeq/stopSeq + late
 * threshold fallback in playRecordStartCue applies. Tauri WebViews are usually lenient about
 * the first resume, so priming typically lets the common path take the synchronous branch.
 * Call once when the capsule window mounts.
 */
export function primeAudioCue(): void {
  // getContext already rebuilds a closed ctx; here we only need to wake suspended/interrupted.
  const ctx = getContext();
  if (!ctx) return;
  if (audioContextActionForState(ctx.state) === 'resume') {
    try {
      void ctx.resume().catch(() => undefined);
    } catch {
      discardContext(ctx);
    }
  }
}

/** Play the "recording started" cue. Silently degrades when Web Audio is unavailable or the context is suspended and cannot be resumed. */
export function playRecordStartCue(): void {
  clearRecoveryTimer();
  const myPlay = ++playSeq;
  const stopAtRequest = stopSeq;
  const requestedAt = nowMs();
  const shouldPlay = () =>
    shouldPlayDeferredCue({
      superseded: myPlay !== playSeq,
      stoppedWhileWaiting: stopSeq !== stopAtRequest,
      elapsedMs: nowMs() - requestedAt,
      lateThresholdMs: DEFERRED_CUE_LATE_THRESHOLD_MS,
    });
  playRecordStartCueOnce(true, myPlay, shouldPlay);
}

// allowRecreate limits "discard the dead ctx and retry" to at most once, avoiding infinite
// recursion on an unwakeable ctx.
function playRecordStartCueOnce(
  allowRecreate: boolean,
  myPlay: number,
  shouldPlay: () => boolean,
): void {
  const ctx = getContext();
  if (!ctx) return;

  const recover = () => {
    if (myPlay !== playSeq || sharedCtx !== ctx) return;
    discardContext(ctx);
    if (allowRecreate && shouldPlay()) playRecordStartCueOnce(false, myPlay, shouldPlay);
  };
  const schedule = () => {
    const startedAt = ctx.currentTime;
    scheduleCueVoices(ctx);
    // Some audio interruptions still report running while the audio clock is frozen;
    // proactively retire the ctx before the next keypress.
    recoveryTimer = setTimeout(
      () => {
        if (ctx.currentTime <= startedAt) recover();
      },
      cueTotalDurationMs(recordStartCueTones()) + 50,
    );
  };

  if (audioContextActionForState(ctx.state) !== 'resume') {
    schedule();
    return;
  }

  // resume may never settle. A timeout also enters the recovery path; a late Promise must not
  // touch the old context again.
  let settled = false;
  const settle = (runningAfterResume: boolean): void => {
    if (settled || myPlay !== playSeq || sharedCtx !== ctx) return;
    settled = true;
    clearRecoveryTimer();
    const action = cueActionAfterResume({
      runningAfterResume,
      shouldPlay: shouldPlay(),
      allowRecreate,
    });
    if (action === 'schedule') {
      schedule();
    } else if (!runningAfterResume) {
      // Even if this round is already late, clear the bad context so the next recording can recover.
      recover();
    }
  };

  recoveryTimer = setTimeout(() => settle(false), RESUME_TIMEOUT_MS);
  try {
    void ctx.resume().then(
      () => settle(ctx.state === 'running'),
      () => settle(false),
    );
  } catch {
    settle(false);
  }
}
