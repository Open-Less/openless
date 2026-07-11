// audioCue 纯函数单测，沿用仓库现有 .test.ts 的轻量自执行断言风格
// （无独立 runner —— 在 tsc 类型检查下编译，必要时可用 tsx 直接跑）。
// 播放/停止依赖 Web Audio 运行时，不在此单测覆盖；这里只钉住可被回归的音符规划。

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
    tones.every(t => t.freq > 0 && t.durationMs > 0),
    'every tone has positive frequency and duration',
  );
  // 指数包络 ramp 不能到 0，峰值必须严格为正，否则 exponentialRampToValueAtTime 抛错。
  assert(
    tones.every(t => t.peakGain > 0 && t.peakGain <= 1),
    'every tone peak gain is within (0, 1]',
  );
  // 第二个音上升（小三度），听感是「叮咚」而非平铺两声。
  assert(tones[1].freq > tones[0].freq, 'second tone rises in pitch');
  // 两音交叠：第二个音在第一个音结束前起，连成一段。
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
  // 挂起期间被更新一轮播放接管 → 不播，避免叠音。
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
  // 没被打断 → 正常播放。
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
  // 修复点：快速录音——resume 期间录音已停，但 resume 很快（未超阈值）→ 仍补响一声。
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
  // 录音已停且 resume 真迟到（超阈值）→ 丢弃，避免提示音姗姗来迟。
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
  // 边界：恰好等于阈值不算迟到（> 才丢），仍补播。
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
  // 「用久了没声音」的回归钉子：closed 的 ctx 必须重建，否则提示音/试听永久静默。
  assertEqual(
    audioContextActionForState('closed'),
    'recreate',
    'closed context must be recreated',
  );
  // running 可直接排期。
  assertEqual(
    audioContextActionForState('running'),
    'ready',
    'running context is ready to schedule',
  );
  // suspended 先 resume 再排期（WKWebView/WebView2 常态）。
  assertEqual(
    audioContextActionForState('suspended'),
    'resume',
    'suspended context needs resume',
  );
  // WebKit 非标准 interrupted（音频会话被抢占）同样需要 resume，不能当 running 直接排期。
  assertEqual(
    audioContextActionForState('interrupted'),
    'resume',
    'interrupted context needs resume',
  );
  // 任何未知非运行态都保守地走 resume（宁可尝试唤醒也不静默漏音）。
  assertEqual(
    audioContextActionForState('some-future-state'),
    'resume',
    'unknown non-running state falls back to resume',
  );
}

{
  // 「用久了没声音」修复的回归钉子：resume() 之后的处置决策（cueActionAfterResume）。
  // resume 成功、ctx 真在跑、且仍该播 → 排期发声。
  assertEqual(
    cueActionAfterResume({ runningAfterResume: true, shouldPlay: true, allowRecreate: true }),
    'schedule',
    'running-after-resume and should-play schedules the cue',
  );
  // 被新一轮播放接管 / 真迟到（shouldPlay=false）→ 丢弃，且不重建（让最新那次处理）。
  assertEqual(
    cueActionAfterResume({ runningAfterResume: true, shouldPlay: false, allowRecreate: true }),
    'drop',
    'superseded or late cue is dropped even when the context is running',
  );
  // 核心修复：resume 被拒、或名义 resolve 但 ctx 仍非 running（runningAfterResume=false）——
  // 只要还该播且可重建，就丢弃坏死 ctx 重试，绝不静默放弃 / 不在冻结时钟上排期。
  assertEqual(
    cueActionAfterResume({ runningAfterResume: false, shouldPlay: true, allowRecreate: true }),
    'recreate-retry',
    'a context that will not wake recreates instead of going permanently silent',
  );
  // 重试一次后仍唤不醒（allowRecreate=false）→ 放弃，避免坏死 ctx 上无限递归。
  assertEqual(
    cueActionAfterResume({ runningAfterResume: false, shouldPlay: true, allowRecreate: false }),
    'drop',
    'second attempt gives up to avoid an infinite recreate loop',
  );
  // 本就不该播时，即使唤不醒也不浪费一次重建。
  assertEqual(
    cueActionAfterResume({ runningAfterResume: false, shouldPlay: false, allowRecreate: true }),
    'drop',
    'no recreate is spent when the cue should not play anyway',
  );
}

// 静默成功难以与「没跑」区分；直接 tsx 跑时给个明确通过信号。
console.log('[audioCue.test] all assertions passed');
