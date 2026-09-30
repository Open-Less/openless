// Unit tests for audioCue's pure functions, following the repo's existing lightweight
// self-executing .test.ts assertion style (no separate runner — compiles under tsc type
// checking and can be run directly with tsx when needed).
// Playback/stop depends on the Web Audio runtime and isn't covered here; these tests only
// pin the note scheduling that can regress.

import {
  audioContextActionForState,
  cueActionAfterResume,
  cueTotalDurationMs,
  recordStartCueTones,
  shouldPlayDeferredCue,
  type CueTone,
} from './audioCue';

function assert(cond: boolean, name: string) {
  if (!cond) throw new Error(`assertion failed: ${name}`);
}

function assertEqual<T>(actual: T, expected: T, name: string) {
  if (actual !== expected) {
    throw new Error(`${name}: expected ${String(expected)}, got ${String(actual)}`);
  }
}

{
  const tones = recordStartCueTones();
  assertEqual(tones.length, 2, 'start cue is a two-tone chime');
  assert(
    tones.every((t) => t.freq > 0 && t.durationMs > 0),
    'every tone has positive frequency and duration',
  );
  // An exponential-envelope ramp must not reach 0; peak gain must be strictly positive or
  // exponentialRampToValueAtTime throws.
  assert(
    tones.every((t) => t.peakGain > 0 && t.peakGain <= 1),
    'every tone peak gain is within (0, 1]',
  );
  // The second tone rises (minor third), so it reads as a "ding-dong" instead of two flat beeps.
  assert(tones[1].freq > tones[0].freq, 'second tone rises in pitch');
  // The two tones overlap: the second starts before the first ends, forming one chime.
  assert(
    tones[1].startMs < tones[0].startMs + tones[0].durationMs,
    'tones overlap into a single chime',
  );
}

{
  const flat: CueTone[] = [
    { freq: 440, startMs: 0, durationMs: 100, peakGain: 0.2 },
    { freq: 880, startMs: 90, durationMs: 170, peakGain: 0.2 },
  ];
  assertEqual(cueTotalDurationMs(flat), 260, 'total duration is last tone end (90 + 170)');
  assertEqual(cueTotalDurationMs([]), 0, 'empty cue has zero duration');
  assert(cueTotalDurationMs(recordStartCueTones()) > 0, 'start cue has positive total duration');
}

{
  // Superseded by a newer playback while suspended → don't play, avoiding stacked cues.
  assertEqual(
    shouldPlayDeferredCue({
      superseded: true,
      stoppedWhileWaiting: false,
      elapsedMs: 10,
      lateThresholdMs: 400,
    }),
    false,
    'superseded cue does not play',
  );
  // Nothing interrupted → play normally.
  assertEqual(
    shouldPlayDeferredCue({
      superseded: false,
      stoppedWhileWaiting: false,
      elapsedMs: 5000,
      lateThresholdMs: 400,
    }),
    true,
    'cue plays when nothing interrupted it',
  );
  // The fixed case: a quick recording — recording already stopped during resume, but the
  // resume was fast (under the threshold) → still play the deferred cue.
  assertEqual(
    shouldPlayDeferredCue({
      superseded: false,
      stoppedWhileWaiting: true,
      elapsedMs: 120,
      lateThresholdMs: 400,
    }),
    true,
    'quick recording still gets a slightly-late cue',
  );
  // Recording stopped and the resume is genuinely late (over the threshold) → drop, so the
  // cue doesn't arrive out of nowhere.
  assertEqual(
    shouldPlayDeferredCue({
      superseded: false,
      stoppedWhileWaiting: true,
      elapsedMs: 800,
      lateThresholdMs: 400,
    }),
    false,
    'genuinely late cue is dropped',
  );
  // Boundary: exactly at the threshold is not late (only > drops), still plays.
  assertEqual(
    shouldPlayDeferredCue({
      superseded: false,
      stoppedWhileWaiting: true,
      elapsedMs: 400,
      lateThresholdMs: 400,
    }),
    true,
    'cue at exactly the threshold still plays',
  );
}

{
  // Regression pin for "no sound after long use": a closed ctx must be recreated, otherwise
  // cues/audition go permanently silent.
  assertEqual(audioContextActionForState('closed'), 'recreate', 'closed context must be recreated');
  // running can schedule directly.
  assertEqual(
    audioContextActionForState('running'),
    'ready',
    'running context is ready to schedule',
  );
  // suspended resumes first, then schedules (the WKWebView/WebView2 norm).
  assertEqual(audioContextActionForState('suspended'), 'resume', 'suspended context needs resume');
  // WebKit's non-standard interrupted (audio session preempted) also needs a resume; it
  // must not be treated as running and scheduled directly.
  assertEqual(
    audioContextActionForState('interrupted'),
    'resume',
    'interrupted context needs resume',
  );
  // Any unknown non-running state conservatively resumes (better to attempt a wake-up than
  // silently miss cues).
  assertEqual(
    audioContextActionForState('some-future-state'),
    'resume',
    'unknown non-running state falls back to resume',
  );
}

{
  // Regression pin for the "no sound after long use" fix: the post-resume disposition
  // decision (cueActionAfterResume).
  // Resume succeeded, ctx is really running, and the cue should still play → schedule.
  assertEqual(
    cueActionAfterResume({ runningAfterResume: true, shouldPlay: true, allowRecreate: true }),
    'schedule',
    'running-after-resume and should-play schedules the cue',
  );
  // Superseded by a newer playback / genuinely late (shouldPlay=false) → drop, and don't
  // recreate (let the latest round handle it).
  assertEqual(
    cueActionAfterResume({ runningAfterResume: true, shouldPlay: false, allowRecreate: true }),
    'drop',
    'superseded or late cue is dropped even when the context is running',
  );
  // Core fix: resume rejected, or nominally resolved but ctx still not running
  // (runningAfterResume=false) — as long as the cue should still play and recreation is
  // allowed, drop the dead ctx and retry; never silently give up or schedule on a frozen
  // clock.
  assertEqual(
    cueActionAfterResume({ runningAfterResume: false, shouldPlay: true, allowRecreate: true }),
    'recreate-retry',
    'a context that will not wake recreates instead of going permanently silent',
  );
  // Still unresponsive after one retry (allowRecreate=false) → give up, avoiding infinite
  // recursion on a dead ctx.
  assertEqual(
    cueActionAfterResume({ runningAfterResume: false, shouldPlay: true, allowRecreate: false }),
    'drop',
    'second attempt gives up to avoid an infinite recreate loop',
  );
  // When the cue shouldn't play anyway, don't waste a recreate even if the ctx won't wake.
  assertEqual(
    cueActionAfterResume({ runningAfterResume: false, shouldPlay: false, allowRecreate: true }),
    'drop',
    'no recreate is spent when the cue should not play anyway',
  );
}

// Silent success is indistinguishable from "never ran"; print an explicit pass signal when
// run directly with tsx.
console.log('[audioCue.test] all assertions passed');
