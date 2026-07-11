// 录音提示音：用 Web Audio API 即时「合成」一段短促上升双音，不打包任何音频文件。
// 提供两个操作：
//   - playRecordStartCue()  播放（按下录音热键、进入 recording 状态时调用）
//   - stopAudioCue()        关闭/停止（离开 recording、或连按热键避免叠音时调用）
//
// 触发点在 capsule 窗口（始终存活、收到 capsule:state 事件）；设置页「试听」也复用同一份。
// 设计原则：任何环境（无 Web Audio、AudioContext 被自动播放策略挂起、单音创建失败）都
// 静默降级，绝不抛错影响录音主流程。

/** 单个正弦音符的合成参数（相对提示音起点）。 */
export interface CueTone {
  /** 频率 (Hz)。 */
  freq: number;
  /** 相对提示音起点的开始时间 (ms)。 */
  startMs: number;
  /** 持续时长 (ms)。 */
  durationMs: number;
  /** 指数包络峰值增益 (0..1)，控制响度。 */
  peakGain: number;
}

// 上升小三度双音 (A5 880Hz → C#6 1108.73Hz)：给「开始录音」一个明确、轻快、不刺耳的听感。
// 两个音轻微交叠，听感连贯成一个「叮咚」而非两声独立 beep。纯数据 → 便于单测。
export function recordStartCueTones(): CueTone[] {
  return [
    { freq: 880, startMs: 0, durationMs: 130, peakGain: 0.16 },
    { freq: 1108.73, startMs: 95, durationMs: 170, peakGain: 0.18 },
  ];
}

/** 提示音总时长 (ms) = 最后一个音的结束时刻。供调用方排期 stop / 试听反馈用。 */
export function cueTotalDurationMs(tones: CueTone[]): number {
  return tones.reduce((max, t) => Math.max(max, t.startMs + t.durationMs), 0);
}

// Safari/WKWebView 旧前缀；用结构化类型而非 any 拿到 webkit 兜底构造器。
type AudioContextCtor = typeof AudioContext;
interface WebkitWindow {
  webkitAudioContext?: AudioContextCtor;
}

// 模块级单例。Tauri 每个窗口是独立 webview = 独立 JS 模块实例，所以 capsule 窗口与
// 设置窗口各自持有一份 ctx / activeVoices，不会互相打架。
let sharedCtx: AudioContext | null = null;
interface ActiveVoice {
  osc: OscillatorNode;
  gain: GainNode;
}
let activeVoices: ActiveVoice[] = [];
// suspended 的 AudioContext 要先 resume（异步）再排期。等待 resume 期间可能发生两类事件，
// 用两个序号分别记，避免两个极端——「快速录音整段丢音」和「录音停了提示音才姗姗来迟」：
//   playSeq —— 每次新的播放请求自增；resume 回来若已被更新一轮播放接管，本次让位（防叠音）。
//   stopSeq —— 每次 stopAudioCue 自增；resume 回来若期间发生过 stop，再按「是否真迟到」决定。
let playSeq = 0;
let stopSeq = 0;

// resume 期间录音已结束（发生过 stop）时，只有当「请求 → resume 完成」耗时超过这个阈值
// 才判定真迟到并丢弃；阈值内仍补响一声——快速点一下录音（resume 没跑完就已结束）也该有反馈。
const DEFERRED_CUE_LATE_THRESHOLD_MS = 400;

// 无 performance（理论兜底，Tauri WebView 里恒有）时回退 0：elapsedMs 恒为 0、永不判迟到，
// 即宁可补播一声也不丢音——与"修复丢音"的初衷一致的安全方向。
function nowMs(): number {
  return typeof performance !== 'undefined' ? performance.now() : 0;
}

/**
 * resume 完成后，判断这次「挂起期间排期」的提示音是否还该播放。纯函数，便于单测：
 * - 被更新一轮播放接管 → 不播（让新的那轮来，避免叠音）；
 * - 等待期间录音已停且已真迟到（超阈值）→ 不播（避免提示音晚到）；
 * - 其余（含「停了但 resume 很快」的快速录音）→ 照常补播。
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

/** 提示音播放前对持有的 AudioContext 该做的处置。 */
export type AudioContextAction = 'ready' | 'resume' | 'recreate';

/**
 * 依据 AudioContext 的运行期状态决定如何处置它。纯函数，便于单测，是「用久了没声音」
 * 这个 bug 的修复核心：
 *  - 'closed'  → 'recreate'：已关闭的 ctx 无法复活（resume() 必拒、createOscillator() 必抛），
 *                必须丢弃重建。长时间使用中频繁录音抢占音频会话，WKWebView/WebView2 的
 *                共享 ctx 可能被系统关闭却从不重建——于是「开始提示音」和设置页「试听」
 *                双双永久静默（两个窗口各自持有一份 ctx，都会这样退化）。
 *  - 'running' → 'ready'：可直接排期合成。
 *  - 其余（'suspended' / WebKit 非标准 'interrupted' / 任何未知非运行态）→ 'resume'：
 *                先异步 resume 唤醒再排期。
 */
export function audioContextActionForState(state: string): AudioContextAction {
  if (state === 'closed') return 'recreate';
  if (state === 'running') return 'ready';
  return 'resume';
}

/** resume() 完成后对提示音的处置。 */
export type CueAction = 'schedule' | 'recreate-retry' | 'drop';

/**
 * resume() 结束后如何处置这次提示音。纯函数，便于单测，是「用久了没声音」修复的判定核心。
 *
 * 关键修复点：长时间使用中，本地 ASR / 录音反复抢占系统音频会话后，WKWebView 的共享
 * AudioContext 常被推进 'suspended' 或 WebKit 非标准的 'interrupted'，此时 resume() 会被拒、
 * 或名义 resolve 了但 ctx 仍非 'running'（currentTime 冻结）。旧逻辑要么把 resume 失败静默
 * 吞掉、要么在冻结的时钟上排期 —— 两条路都让提示音永久静默直到进程重启。这里把这两种
 * 「唤不醒」的退化态统一判为「丢弃坏死 ctx、用全新 ctx 重试一次」，让共享 ctx 自愈。
 * （'closed' 早在 getContext 就重建掉了，不会走到这里。）
 *  - resume 成功且 ctx 真在跑：还该播 → 'schedule'，否则 'drop'（被新一轮接管 / 真迟到）。
 *  - 否则（被拒 / resolve 了仍非 running）：允许重建 → 'recreate-retry'；不允许（已重试过一次）
 *    → 'drop'，避免在坏死 ctx 上无限递归。
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
  // window.AudioContext 来自全局声明；webkit 前缀单独用结构化类型拿，避免 any。
  const webkit = window as WebkitWindow;
  return window.AudioContext ?? webkit.webkitAudioContext ?? null;
}

function getContext(): AudioContext | null {
  const Ctor = resolveAudioContextCtor();
  if (!Ctor) return null;
  // 已 closed 的 ctx 无法复活，丢弃后重建——否则后续所有提示音永久静默（本 bug 根因）。
  // 旧 ctx 上挂着的 activeVoices 也一并清空，避免去叠音时操作已失效节点。
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

// 丢弃一个唤不醒 / 坏死的 AudioContext：清空模块单例（下次 getContext 会重建全新一份）与挂在
// 其上的 activeVoices，并尽力 close 释放底层音频资源。已 closed / close 失败都忽略。
function discardContext(ctx: AudioContext): void {
  if (sharedCtx === ctx) {
    sharedCtx = null;
  }
  activeVoices = [];
  try {
    void ctx.close().catch(() => undefined);
  } catch {
    // 已 closed，或实现未返回 Promise，忽略。
  }
}

// 停掉当前正在发声的节点（不动 playSeq / stopSeq —— 仅做去叠音 / 收尾）。
function stopVoices(): void {
  const ctx = sharedCtx;
  const now = ctx?.currentTime ?? 0;
  for (const { osc, gain } of activeVoices) {
    try {
      gain.gain.cancelScheduledValues(now);
      // 指数 ramp 不能到 0，用极小值做近似静音后立即停振。
      gain.gain.setValueAtTime(0.0001, now);
      osc.stop(now + 0.02);
    } catch {
      // 已停止 / 已断开，忽略。
    }
  }
  activeVoices = [];
}

/** 关闭/停止提示音：停掉在播节点，并标记「期间发生过 stop」，供挂起的 resume 回调判定迟到。 */
export function stopAudioCue(): void {
  stopSeq++;
  stopVoices();
}

// 实际排期合成节点。必须在 AudioContext 处于 running（非 suspended）时调用：
// suspended 时 currentTime 冻结在暂停时刻，节点会排到过期时间点 → 不发声还堆积。
function scheduleCueVoices(ctx: AudioContext): void {
  // 连按热键时先停掉上一轮，避免叠音越来越响。用 stopVoices 而非 stopAudioCue：
  // 这里不该作废自己这一轮的 generation。
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
      // 5ms attack + 指数 release：避免起停的 click 爆音。
      gain.gain.setValueAtTime(0.0001, t0);
      gain.gain.exponentialRampToValueAtTime(tone.peakGain, t0 + 0.005);
      gain.gain.exponentialRampToValueAtTime(0.0001, tEnd);
      osc.connect(gain).connect(ctx.destination);
      osc.start(t0);
      osc.stop(tEnd + 0.02);

      const voice: ActiveVoice = { osc, gain };
      activeVoices.push(voice);
      osc.onended = () => {
        activeVoices = activeVoices.filter(v => v !== voice);
        try {
          osc.disconnect();
          gain.disconnect();
        } catch {
          // noop
        }
      };
    } catch {
      // 单个音创建/排期失败不影响其余音。
    }
  }
}

/**
 * 预热 AudioContext：尽早创建并 resume，让录音真正开始时 ctx 已是 running，
 * playRecordStartCue 能同步排期，绕开 suspended→resume 的异步竞态——快速录音丢音的根因。
 * 注：严格 autoplay policy 下无用户手势的 resume 可能被拒（已静默降级），此时预热不生效，
 * 退回 playRecordStartCue 里的 playSeq/stopSeq + 迟到阈值兜底。Tauri WebView 多对首次
 * resume 宽松，预热通常能让常见路径走同步分支。胶囊窗口挂载时调用一次即可。
 */
export function primeAudioCue(): void {
  // getContext 会把 closed 的 ctx 重建掉；这里只需把 suspended/interrupted 唤醒。
  const ctx = getContext();
  if (!ctx) return;
  if (audioContextActionForState(ctx.state) === 'resume') {
    ctx.resume().catch(() => undefined);
  }
}

/** 播放「开始录音」提示音。无 Web Audio 或被挂起且无法恢复时静默降级。 */
export function playRecordStartCue(): void {
  playRecordStartCueOnce(true);
}

// allowRecreate 把「丢弃坏死 ctx 并重试」限制为最多一次，避免在唤不醒的 ctx 上无限递归。
function playRecordStartCueOnce(allowRecreate: boolean): void {
  const ctx = getContext();
  if (!ctx) return;

  // ctx 已 running 直接排期；closed 已在 getContext 重建。其余（suspended / WebKit 非标准
  // interrupted / 未知态）必须先 resume 再排期，不能在 resume 未完成时就用冻结的 currentTime。
  if (audioContextActionForState(ctx.state) !== 'resume') {
    scheduleCueVoices(ctx);
    return;
  }

  const myPlay = ++playSeq;
  const stopAtRequest = stopSeq;
  const requestedAt = nowMs();

  // resume 完成（成功或被拒）后统一决策：排期 / 丢弃重建重试 / 放弃。
  const settle = (runningAfterResume: boolean): void => {
    const action = cueActionAfterResume({
      runningAfterResume,
      // 被更新一轮播放接管就让位；期间录音已停且 resume 真迟到才丢弃；否则照常补响——
      // 快速点一下录音（resume 没跑完就已结束）也该有提示音。
      shouldPlay: shouldPlayDeferredCue({
        superseded: myPlay !== playSeq,
        stoppedWhileWaiting: stopSeq !== stopAtRequest,
        elapsedMs: nowMs() - requestedAt,
        lateThresholdMs: DEFERRED_CUE_LATE_THRESHOLD_MS,
      }),
      allowRecreate,
    });
    if (action === 'schedule') {
      scheduleCueVoices(ctx);
    } else if (action === 'recreate-retry') {
      // resume 被拒、或 resolve 了但 ctx 仍非 running（被音频会话抢占后卡死）——「用久了没
      // 声音」的根因。丢弃这个唤不醒的 ctx，用全新 ctx 重试一次，让共享 ctx 自愈。
      discardContext(ctx);
      playRecordStartCueOnce(false);
    }
    // 'drop'：被接管 / 真迟到 / 重试后仍唤不醒——静默放弃。
  };

  ctx
    .resume()
    .then(() => settle(ctx.state === 'running'))
    .catch(() => settle(false));
}
