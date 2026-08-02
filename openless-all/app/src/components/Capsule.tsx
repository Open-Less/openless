import { useEffect, useMemo, useRef, useState, type CSSProperties } from 'react';
import { useTranslation } from 'react-i18next';
import { detectOS, type OS } from './WindowChrome';
import { SiriGL, warmUpSiriShaders } from './SiriGL';
import {
  getCapsuleHostMetrics,
  getCapsulePillMetrics,
} from '../lib/capsuleLayout';
import { isTauri } from '../lib/ipc';
import type { CapsulePayload, CapsuleState } from '../lib/types';

// 胶囊 keyframes 注入一次到 document.head，而不是放在组件 JSX 里。否则录音时音量
// 每帧（~60Hz）setLevel 都会让 React 重新创建/reconcile 这个 <style> 元素 —— 纯属
// 浪费，因为这些 keyframes 是静态的。与 QaPanel / LessComputerPanel 注入方式一致。
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
  /* 思考圆点（fluid dots）出场：纯淡入接住光条汇聚成的光点 —— 圆点自身的
     「从圆心散开」动画由 shader 的 uGather 驱动，这里不做任何缩放冲击。 */
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

interface VoiceOrbStageProps {
  os: OS;
  state: CapsuleState;
  level: number;
  /** 预备态：录音光条渲染成「待命」呼吸形态，不接真实电平。见 CapsulePayload.warming。 */
  warming?: boolean;
  /** 预备→就绪的平均耗时（ms），驱动展开动画的预测节奏。见 SiriGL warmupMs。 */
  warmupMs?: number;
  message?: string;
}

/**
 * 纯光效舞台（siri-glsl 完整克隆，无壳无按钮无底）：
 *   - recording：彩虹光谱声波横贯舞台，振幅随真实麦克风电平起伏；
 *   - transcribing / polishing：波形从两端向中间收缩汇聚，流体圆点环淡入加速转动；
 *   - done / cancelled：转速回落标准，六点合并成中央一颗圆，由外层 capsule-out 淡出；
 *   - error：冻结光效 + 浮一行发光红字说明原因（唯一保留的文字信息）。
 * 刻意没有任何垫底/暗晕（用户拍板）：白底界面上宁可对比度弱，也不要黑色遮挡。
 */
function VoiceOrbStage({ os, state, level, warming, warmupMs, message }: VoiceOrbStageProps) {
  const { t } = useTranslation();
  const metrics = useMemo(() => getCapsulePillMetrics(os), [os]);

  // done / cancelled / error 冻结最后形态淡出，不再切换 phase。
  const lastPhaseRef = useRef<'wave' | 'orb'>('wave');
  let phase = lastPhaseRef.current;
  if (state === 'recording') phase = 'wave';
  else if (state === 'transcribing' || state === 'polishing') phase = 'orb';
  lastPhaseRef.current = phase;
  const isOrb = phase === 'orb';

  // 性能：波形淡出彻底结束（.55s delay + .6s duration）后卸载它的绘制循环 ——
  // 思考期间不再为一块不可见的 canvas 每帧跑 fragment。回到录音态立即重挂
  //（shader 编译已被驱动缓存，重建近零耗时）。
  const [waveAlive, setWaveAlive] = useState(true);
  useEffect(() => {
    if (!isOrb) {
      setWaveAlive(true);
      return undefined;
    }
    const timer = setTimeout(() => setWaveAlive(false), 1300);
    return () => clearTimeout(timer);
  }, [isOrb]);

  return (
    <div
      style={{
        width: metrics.width,
        height: metrics.height,
        boxSizing: metrics.boxSizing,
        fontFamily: 'var(--ol-font-sans)',
        position: 'relative',
        pointerEvents: 'none',
      }}
    >
      {waveAlive && (
        <SiriGL
          mode="wave"
          level={level}
          resolved={!isOrb}
          warming={warming}
          warmupMs={warmupMs}
          style={{
            position: 'absolute',
            inset: 0,
            width: '100%',
            height: '100%',
            // 收缩汇聚进行时波形保持可见，收成中央光点后再淡出，与圆点环的淡入交叠。
            opacity: isOrb ? 0 : 1,
            transition: isOrb ? 'opacity .6s ease-out .55s' : 'opacity .25s ease-out',
          }}
        />
      )}
      {isOrb && (
        <SiriGL
          mode="orb"
          // 思考中（LLM 接收）加速转动；插入/取消/出错时回落到标准速度，
          // 同时六点合并成中央一颗圆，随外层淡出一起消失。
          speed={state === 'transcribing' || state === 'polishing' ? 1.5 : 1.0}
          merging={state !== 'transcribing' && state !== 'polishing'}
          style={{
            position: 'absolute',
            left: '50%',
            top: '50%',
            width: 170,
            height: 170,
            marginLeft: -85,
            marginTop: -85,
            animation: 'siri-orb-in .7s ease-out .3s both',
          }}
        />
      )}
      {state === 'error' && (
        <span style={errorGlowTextStyle}>{message || t('capsule.error')}</span>
      )}
    </div>
  );
}

const errorGlowTextStyle: CSSProperties = {
  position: 'absolute',
  bottom: 24,
  left: '50%',
  transform: 'translateX(-50%)',
  maxWidth: 400,
  fontSize: 12,
  fontWeight: 600,
  lineHeight: 1.4,
  textAlign: 'center',
  color: '#ff7a70',
  textShadow: '0 0 14px rgba(255,70,60,0.5), 0 1px 6px rgba(0,0,0,0.6)',
  whiteSpace: 'nowrap',
  overflow: 'hidden',
  textOverflow: 'ellipsis',
};

interface SelectionPolishNoticeProps {
  state: CapsuleState;
  message?: string;
}

/**
 * 选区润色专用的一行无交互提示。它处在同一扇 Windows no-activate + mouse-passthrough
 * capsule 窗口中，因此不会把焦点从用户当前输入框抢走，也不会遮挡点击。
 */
function SelectionPolishNotice({ state, message }: SelectionPolishNoticeProps) {
  const { t } = useTranslation();
  const processing = state === 'polishing';
  const failed = state === 'error';
  const completed = state === 'done';
  const cancelled = state === 'cancelled';
  const label = message
    ?? (processing
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

// 与 @keyframes capsule-out 的 0.52s 时长一致——必须同步，否则定时器先于
// 动画结束就 unmount → 用户看到半截动画被截断。
const EXIT_ANIM_MS = 520;

// 胶囊「预备→就绪」的平均耗时（ms），驱动入场展开动画的预测节奏（见 SiriGL warmProgress）：
// 每次录音就绪后按 EMA 更新并存 localStorage 跨会话学习。默认 150ms，clamp [60,600]。
const WARMUP_MS_KEY = 'ol-capsule-warmup-ms';
const WARMUP_MS_DEFAULT = 150;
function readWarmupMs(): number {
  if (typeof localStorage === 'undefined') return WARMUP_MS_DEFAULT;
  const raw = Number(localStorage.getItem(WARMUP_MS_KEY));
  return Number.isFinite(raw) && raw > 0 ? Math.min(600, Math.max(60, raw)) : WARMUP_MS_DEFAULT;
}
// #470 诊断 v2：模块级一次性门，只在 webview 收到第一个 capsule:state 事件时打 log。
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

function getPreviewCapsulePayload() {
  if (isTauri || typeof window === 'undefined') {
    return {
      state: 'idle' as CapsuleState,
      level: 0,
      message: undefined,
      translation: false,
      warming: false,
      selectionPolish: false,
    };
  }

  const params = new URLSearchParams(window.location.search);
  const stateParam = params.get('state');
  const previewState = CAPSULE_PREVIEW_STATES.includes(stateParam as CapsuleState)
    ? (stateParam as CapsuleState)
    : 'recording';
  const previewLevel = Number(params.get('level') ?? 0.6);
  return {
    state: previewState,
    level: Number.isFinite(previewLevel) ? Math.min(1, Math.max(0, previewLevel)) : 0.6,
    message: params.get('message') ?? undefined,
    translation: params.get('translation') === '1',
    warming: params.get('warming') === '1',
    selectionPolish: params.get('selectionPolish') === '1',
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
  const [translation, setTranslation] = useState<boolean>(preview.translation);
  const [selectionPolish, setSelectionPolish] = useState<boolean>(preview.selectionPolish);
  // 预备态：麦克风尚未吐第一帧 PCM。true 时录音光条走「待命」呼吸形态（见 SiriGL warming）。
  const [warming, setWarming] = useState<boolean>(preview.warming);
  // 预备→就绪耗时的移动平均，驱动光条展开动画的预测节奏（见 SiriGL warmProgress）。
  const [warmupMs, setWarmupMs] = useState<number>(() => readWarmupMs());
  const warmStartRef = useRef<number | null>(null);
  // `leaving` 与 `lastVisibleState` 协同实现「退出动画」：
  // - 当 state 从非 idle 变成 idle 时，不立即卸载，而是把 leaving 置为 true 并保留
  //   最后一帧的可见 state（lastVisibleState），让语音 orb 用 capsule-out 动画收缩淡出。
  // - 动画结束（EXIT_ANIM_MS）后再把 leaving 置回 false，组件回到「真正未挂载」分支。
  // - 若期间 state 又切回非 idle（例如用户连按热键），立刻中止 leaving 并恢复显示。
  const [leaving, setLeaving] = useState<boolean>(false);
  const [lastVisibleState, setLastVisibleState] = useState<CapsuleState>(preview.state);
  const [lastVisibleSelectionPolish, setLastVisibleSelectionPolish] = useState<boolean>(
    preview.selectionPolish,
  );
  const [lastVisibleMessage, setLastVisibleMessage] = useState<string | undefined>(preview.message);
  // 前端 host 与原生窗口保持同一份透明语音 orb 舞台尺寸。
  const hostMetrics = getCapsuleHostMetrics(os, translation);
  const badgeBottom = Math.round(metrics.height * 0.73);

  // 空闲预热 shader 编译缓存：首次按下热键时光条不用现场编译（响应延迟反馈）。
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
      const handle = await listen<CapsulePayload>('capsule:state', event => {
        const p = event.payload;
        if (!capsuleStateFirstLogged) {
          capsuleStateFirstLogged = true;
          // #470 诊断 v2：确认 capsule webview 确实收到了后端事件 —— 区分「后端没
          // emit」与「emit 了但窗口没显示/没渲染」。配合后端 [capsule] 日志定位根因。
          console.info('[capsule] first capsule:state received in webview, state=', p.state);
        }
        setState(p.state);
        setLevel(p.level ?? 0);
        setMessage(p.message ?? undefined);
        setTranslation(p.translation === true);
        setWarming(p.warming === true);
        setSelectionPolish(p.selectionPolish === true);
      });
      if (cancelled) handle();
      else unlisten = handle;
    })();
    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, []);

  // 退出动画调度：在 state 真正进入 idle 时，先用 capsule-out 播放 EXIT_ANIM_MS，再卸载。
  // 设计要点：
  // 1. 进入非 idle：清掉 leaving，记录最新可见 state；
  // 2. 进入 idle 且之前可见：开启 leaving 并启动定时器；
  // 3. 期间又被打回非 idle：cleanup 直接 clearTimeout，定时器不会触发，
  //    新一轮 effect 会立即恢复可见态，避免错误地把可见状态切到 idle。
  useEffect(() => {
    if (state !== 'idle') {
      // 立即恢复可见，并取消上一轮可能挂着的离场。
      if (leaving) setLeaving(false);
      setLastVisibleState(state);
      return undefined;
    }
    // state === 'idle'：判断是不是从可见态过渡过来。
    if (lastVisibleState === 'idle') return undefined;
    setLeaving(true);
    const timer = setTimeout(() => {
      setLeaving(false);
      setLastVisibleState('idle');
    }, EXIT_ANIM_MS);
    return () => clearTimeout(timer);
    // 故意只依赖 state —— lastVisibleState / leaving 是内部派生量，
    // 把它们加进依赖会让定时器被反复重建。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state]);

  // Idle payload 不携带终态文案。保留最近一帧可见 selection 提示，才能让 auto-hide
  // 触发后的 520ms 离场动画继续显示“已替换 / 未选中内容”等正确反馈，而不闪回语音舞台。
  useEffect(() => {
    if (state === 'idle') return;
    setLastVisibleSelectionPolish(selectionPolish);
    setLastVisibleMessage(message);
  }, [state, selectionPolish, message]);

  // 学习「预备态持续多久」= 麦克风就绪耗时，EMA 更新 warmupMs（预测展开的基准时长）。
  useEffect(() => {
    if (warming) {
      warmStartRef.current = performance.now();
      return;
    }
    if (warmStartRef.current == null) return;
    const loadMs = performance.now() - warmStartRef.current;
    warmStartRef.current = null;
    // 过滤异常样本：<20ms 多半不是真入场；>3s 多半首次 TCC / 卡顿，不代表常态。
    if (loadMs < 20 || loadMs > 3000) return;
    setWarmupMs(prev => {
      const next = Math.min(600, Math.max(60, prev * 0.7 + loadMs * 0.3));
      try { localStorage.setItem(WARMUP_MS_KEY, String(Math.round(next))); } catch { /* ignore */ }
      return next;
    });
  }, [warming]);

  // 真正卸载：state 已是 idle，且不在离场动画中。
  if (state === 'idle' && !leaving) {
    return <div style={{ width: 0, height: 0 }} />;
  }

  // 离场时用 lastVisibleState 渲染最后一帧内容，避免把 idle 当作无波形状态。
  const renderedState: CapsuleState = state === 'idle' ? lastVisibleState : state;
  const renderedSelectionPolish = state === 'idle'
    ? lastVisibleSelectionPolish
    : selectionPolish;
  const renderedMessage = state === 'idle' ? lastVisibleMessage : message;

  return (
    <div
      style={{
        width: '100%',
        height: '100%',
        position: 'relative',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        paddingLeft: hostMetrics.horizontalInset,
        paddingRight: hostMetrics.horizontalInset,
        boxSizing: hostMetrics.boxSizing,
        paddingTop: os === 'win'
          ? Math.max(0, hostMetrics.height - metrics.height - hostMetrics.bottomInset)
          : 0,
        paddingBottom: os === 'win' ? hostMetrics.bottomInset : 0,
        background: 'transparent',
        animation: leaving
          ? 'capsule-out .52s cubic-bezier(.55,.06,.68,.19) forwards'
          // .68s 的入场被反馈「按下之后有延迟」：压到 .38s，曲线保持轻微弹性，
          // 光条几乎跟手出现。
          : 'capsule-in .38s cubic-bezier(.3,1.2,.4,1) both',
        transformOrigin: 'center',
        willChange: 'transform, opacity',
      }}
    >
      {!renderedSelectionPolish && (
        <>
      {/* "正在翻译" 徽章 — 嵌套两层：
          外层只负责"绝对定位 + 水平居中（translateX(-50%)）"，不参与动画；
          内层只负责"垂直位移 + 渐变透明度"——这样不会跟 translateX(-50%) 冲突，
          也不存在 keyframe 与 inline transform 互相覆盖导致的视觉跳变。 */}
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
            // 纯光效语言：无壳发光小字，与波形同一气质（浮空 + 冷蓝光晕）。
            color: 'rgba(190, 212, 255, 0.95)',
            textShadow: '0 0 14px rgba(90, 140, 255, 0.85), 0 1px 6px rgba(0, 0, 0, 0.45)',
            letterSpacing: '0.02em',
            whiteSpace: 'nowrap',
            // 隐藏：从光条附近偏下出发；显示：归位到光条上方。
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
      <VoiceOrbStage
        os={os}
        state={renderedState}
        level={leaving ? 0 : level}
        warming={!leaving && warming}
        warmupMs={warmupMs}
        message={renderedMessage}
      />
        </>
      )}
      {renderedSelectionPolish && (
        <SelectionPolishNotice state={renderedState} message={renderedMessage} />
      )}
    </div>
  );
}
