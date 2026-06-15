import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { detectOS, type OS } from './WindowChrome';
import {
  getCapsuleHostMetrics,
  getCapsuleMessageLayout,
  getCapsulePillMetrics,
} from '../lib/capsuleLayout';
import { invokeOrMock, isTauri } from '../lib/ipc';
import type { CapsulePayload, CapsuleState } from '../lib/types';

// 胶囊 keyframes 注入一次到 document.head，而不是放在组件 JSX 里。否则录音时音量
// 每帧（~60Hz）setLevel 都会让 React 重新创建/reconcile 这个 <style> 元素 —— 纯属
// 浪费，因为这些 keyframes 是静态的。与 QaPanel / LessComputerPanel 注入方式一致。
const CAPSULE_KEYFRAMES = `
  /* 入场：从中央很窄的一小条（scaleX 0.18）+ 略压扁（scaleY 0.95）+ 透明，
     长出到 scaleX 1 / scaleY 1 / 不透明。配合 wrapper 的 transformOrigin:center，
     视觉上是「从中心向左右展开」。 */
  @keyframes capsule-in {
    from { opacity: 0; transform: scale(.78) translateY(8px); }
    to   { opacity: 1; transform: scale(1)   translateY(0); }
  }
  /* 离场：scaleX 由 1 收回 0.18 + 整体向下偏移 8px + 淡出。
     forwards 让最终帧（opacity:0、scaleX:.18）保持到组件被卸载。 */
  @keyframes capsule-out {
    from { opacity: 1; transform: scaleX(1)   translateY(0); }
    to   { opacity: 0; transform: scaleX(.18) translateY(8px); }
  }
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

interface AudioBarsProps {
  level: number;
}

function AudioBars({ level }: AudioBarsProps) {
  const envelope = [0.55, 0.85, 1.0, 0.85, 0.55];
  const base = 2;
  const max = 24;
  const voice = Math.min(1, Math.max(0, level));
  const silenceGate = 0.012;
  const responseCeiling = 0.34;
  const gatedVoice = Math.min(1, Math.max(0, (voice - silenceGate) / (responseCeiling - silenceGate)));
  const easedVoice = gatedVoice * gatedVoice * (3 - 2 * gatedVoice);
  const visualVoice = Math.pow(easedVoice, 0.42);
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        gap: 3,
        width: 42,
        height: max,
      }}
    >
      {envelope.map((env, i) => (
        <span
          key={i}
          style={{
            display: 'inline-block',
            width: 3,
            height: base + (max - base) * visualVoice * env,
            borderRadius: 999,
            background: 'var(--ol-blue)',
            opacity: 0.82,
            transformOrigin: 'center',
            // 0.08s 在 60Hz audio-level 更新下太快，每次 re-render 都重启 transition，
            // 视觉上是阶梯式跳变。延长到 0.18s 让多次 update 在曲线内平滑混合，
            // easeOutExpo-like 缓动让圆点→长条的形变自然顺滑（用户原话"圆形跳成矩形"）。
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
  const metrics = getCapsulePillMetrics(os);
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

// memo:录音时 level 每帧(~60Hz)变化会重渲 Pill;cancel/confirm 两个 SVG 按钮跟
// level 无关,memo + 稳定的 onClick 让它们在录音期间跳过重渲(只剩音量条真正更新)。
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
        transition: 'opacity 0.18s var(--ol-motion-soft), background 0.16s var(--ol-motion-quick), transform 0.12s var(--ol-motion-quick)',
      }}
    >
      {isCancel ? (
        <svg width="11" height="11" viewBox="0 0 11 11">
          <path d="M1.5 1.5l8 8M9.5 1.5l-8 8" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
        </svg>
      ) : (
        <svg width="13" height="13" viewBox="0 0 13 13">
          <path d="M2 6.5l3.2 3.5L11 3.5" stroke="currentColor" strokeWidth="1.7" fill="none" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      )}
    </button>
  );
});

interface PillProps {
  os: OS;
  state: CapsuleState;
  level: number;
  insertedChars: number;
  message?: string;
  operating?: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}

function Pill({ os, state, level, insertedChars, message, operating, onCancel, onConfirm }: PillProps) {
  const { t } = useTranslation();
  const metrics = useMemo(() => getCapsulePillMetrics(os), [os]);
  const processingLayout = useMemo(() => getCapsuleMessageLayout(os, 'processing'), [os]);
  const cancelEnabled = state === 'recording' || state === 'transcribing' || state === 'polishing';
  const confirmEnabled = state === 'recording';

  // "thinking" 扫光速度：进入 transcribing/polishing 的头 2 秒走快速（0.9s/cycle，提示
  // 「流式刚开始」），之后切回慢速（2.4s）作为稳态。切回 idle / done / 其他 state 也复位
  // 为 fast，下次进入时从头开始 burst。
  const [shineFast, setShineFast] = useState(true);
  useEffect(() => {
    if (state === 'transcribing' || state === 'polishing') {
      setShineFast(true);
      const t = setTimeout(() => setShineFast(false), 2000);
      return () => clearTimeout(t);
    }
    setShineFast(true);
    return undefined;
  }, [state]);

  let center: JSX.Element;
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
            // 左右 4px 内边距 + 外层 gap 已经让 "thinking" ↔ ✗/✓ 视觉间距落在 ~4-5px。
            padding: '0 4px',
            width: '100%',
            maxWidth: metrics.textWidth,
            minWidth: 0,
            justifyContent: 'center',
            // state 进入动画 —— 用户从 recording 切到 polishing 时多一道淡入提示，
            // 比纯切换 center 内容更容易被感知。
            animation: 'cap-state-enter 220ms var(--ol-motion-soft) both',
          }}
        >
          <span
            style={{
              // v1.3.1-7 用户拍板：黑色底字 + 蓝色扫光（亮黄太显眼，黑底更稳）。
              // 字号保持 17，字重 700 → 600 稍细一些。
              fontSize: 17,
              fontWeight: 600,
              letterSpacing: 0.3,
              // line-height: 1 下 g/y/p 等下伸字符会被 clip，给 padding 留 descender 空间。
              paddingBlock: 1,
              color: 'var(--ol-ink-2)',
              backgroundImage:
                'linear-gradient(100deg, var(--ol-ink) 0%, var(--ol-ink) 35%, var(--ol-blue) 50%, var(--ol-ink) 65%, var(--ol-ink) 100%)',
              backgroundSize: '220% auto',
              WebkitBackgroundClip: 'text',
              backgroundClip: 'text',
              WebkitTextFillColor: 'transparent',
              // 进入流式的头 ~2 秒用 0.9s 高速扫光（视觉提示「刚开始」），之后 React 副作用
              // 切到 2.4s 慢速。duration 变化时浏览器不重启动画，会平滑减速。
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
            {t(operating ? 'capsule.using' : 'capsule.thinking')}
          </span>
        </div>
      );
      break;
    case 'done':
      center = <CenterText os={os} kind="default" text={message || t('capsule.inserted', { count: insertedChars })} />;
      break;
    case 'cancelled':
      center = <CenterText os={os} kind="default" text={t('capsule.cancelled')} />;
      break;
    case 'error':
      center = <CenterText os={os} kind="error" text={message || t('capsule.error')} color="var(--ol-err)" />;
      break;
    default:
      center = <AudioBars level={0} />;
  }

  const ambient = state === 'recording' ? Math.min(1, Math.max(0, level)) : 0;
  const scale = os === 'win' ? 1 : 1 + ambient * 0.018;
  const shadowAlpha = 0.20 + ambient * 0.10;

  return (
    // 非 Linux 走假毛玻璃；Linux 禁用透明窗口后由 .ol-frost 平台规则退成不透明面。
    // 不写 backdrop-filter —— webview 模糊不了透明窗口背后的桌面（Tauri 上游限制）。
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
        boxSizing: metrics.boxSizing,
        borderRadius: 999,
        border: '1px solid var(--ol-capsule-pill-border)',
        boxShadow: `${os === 'win' ? `0 10px 24px -14px rgba(0, 0, 0, ${(0.24 + ambient * 0.06).toFixed(3)})` : `0 18px 50px -10px rgba(0, 0, 0, ${shadowAlpha.toFixed(3)})`}, 0 0 0 0.5px rgba(0, 0, 0, 0.24), var(--ol-capsule-pill-inset)`,
        color: 'var(--ol-capsule-center-ink)',
        fontFamily: 'var(--ol-font-sans)',
        transform: `scale(${scale.toFixed(4)})`,
        transformOrigin: 'center',
        transition: 'transform 0.08s var(--ol-motion-quick), box-shadow 0.08s var(--ol-motion-quick)',
        willChange: 'transform, box-shadow',
      }}
    >
      <CircleButton variant="cancel" enabled={cancelEnabled} onClick={onCancel} />
      <div style={{ flex: 1, minWidth: 0, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
        {center}
      </div>
      <CircleButton variant="confirm" enabled={confirmEnabled} onClick={onConfirm} />
    </div>
  );
}

// 与 @keyframes capsule-out 的 0.36s 时长一致——必须同步，否则定时器先于
// 动画结束就 unmount → 用户看到半截动画被截断。
// v1.3.1-6: 从 240ms 加到 360ms 让用户看清退出动画（240ms 太快感知不到）。
const EXIT_ANIM_MS = 360;
// #470 诊断 v2：模块级一次性门，只在 webview 收到第一个 capsule:state 事件时打 log。
let capsuleStateFirstLogged = false;

// 初始可见 state：Tauri 内运行从 idle 开始（等后端 capsule:state 事件），
// 浏览器 dev 模式从 recording 开始以便直接看到胶囊。
const INITIAL_VISIBLE_STATE: CapsuleState = isTauri ? 'idle' : 'recording';

export function Capsule() {
  const { t } = useTranslation();
  const os = detectOS();
  const metrics = getCapsulePillMetrics(os);
  const [state, setState] = useState<CapsuleState>(INITIAL_VISIBLE_STATE);
  const [level, setLevel] = useState<number>(isTauri ? 0 : 0.6);
  const [insertedChars, setInsertedChars] = useState<number>(0);
  const [message, setMessage] = useState<string | undefined>();
  const [translation, setTranslation] = useState<boolean>(false);
  const [operating, setOperating] = useState<boolean>(false);
  // `leaving` 与 `lastVisibleState` 协同实现「退出动画」：
  // - 当 state 从非 idle 变成 idle 时，不立即卸载，而是把 leaving 置为 true 并保留
  //   最后一帧的可见 state（lastVisibleState），让胶囊用 capsule-out 动画收缩淡出。
  // - 动画结束（EXIT_ANIM_MS）后再把 leaving 置回 false，组件回到「真正未挂载」分支。
  // - 若期间 state 又切回非 idle（例如用户连按热键），立刻中止 leaving 并恢复显示。
  const [leaving, setLeaving] = useState<boolean>(false);
  const [lastVisibleState, setLastVisibleState] = useState<CapsuleState>(INITIAL_VISIBLE_STATE);
  // Windows 端 host 在翻译模式从 84 长到 118；macOS / Linux 上 capsuleLayout 已固定 42 忽略此参数。
  const hostMetrics = getCapsuleHostMetrics(os, translation);

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
        if (p.insertedChars != null) setInsertedChars(p.insertedChars);
        setTranslation(p.translation === true);
        setOperating(p.operating === true);
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

  const onCancel = useCallback(() => {
    void invokeOrMock<void>('cancel_dictation', undefined, () => undefined);
  }, []);

  const onConfirm = useCallback(() => {
    void invokeOrMock<void>('stop_dictation', undefined, () => undefined);
  }, []);

  // 真正卸载：state 已是 idle，且不在离场动画中。
  if (state === 'idle' && !leaving) {
    return <div style={{ width: 0, height: 0 }} />;
  }

  // 离场时用 lastVisibleState 渲染最后一帧内容，避免把 idle 当作 fallback 走到 AudioBars(0)。
  const renderedState: CapsuleState = state === 'idle' ? lastVisibleState : state;

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
        // 入场：中央 scaleX 由 0.18 长到 1（视觉上像从中心向两端展开）+ 淡入。
        // 离场：scaleX 由 1 收缩回 0.18 + 向下偏移 8px + 淡出。
        // 三平台一致 —— 旧版 Windows 走 animation:'none' 的分支已删除。
        // transformOrigin 默认就是 50% 50%，所以 scaleX 天然以中央为锚点。
        animation: leaving
          // v1.3.1-6 调整：
          // - 入场 .26s → .38s，cubic-bezier 加强 spring overshoot（更曲线感）
          // - 出场 .24s → .36s（前面 EXIT_ANIM_MS 也同步到 360），曲线改成 ease-in-out 平滑
          //   收缩 + 下移 + 淡出三段同步进行
          ? 'capsule-out .36s cubic-bezier(.55,.06,.68,.19) forwards'
          // 入场改成弹性"弹出"：整体 scale 从 .78 弹到 1，back-out 曲线带回弹 overshoot。
          : 'capsule-in .46s cubic-bezier(.34,1.56,.64,1) both',
        transformOrigin: 'center',
        willChange: 'transform, opacity',
      }}
    >
      {/* "正在翻译" 徽章 — 嵌套两层：
          外层只负责"绝对定位 + 水平居中（translateX(-50%)）"，不参与动画；
          内层只负责"垂直位移 + 渐变透明度"——这样不会跟 translateX(-50%) 冲突，
          也不存在 keyframe 与 inline transform 互相覆盖导致的视觉跳变。 */}
      <div
        style={{
          position: 'absolute',
          left: '50%',
          // macOS / Linux：胶囊窗口 220×110、pill 居中，badge 锚到 pill 中线上方 21+8。
          // Windows：host 比 pill 多出左右 12px / 底部 12px 的阴影空间，pill 仍保持居中。
          bottom: os === 'win'
            ? `${hostMetrics.bottomInset + metrics.height + hostMetrics.badgeGap}px`
            : 'calc(50% + 21px + 8px)',
          transform: 'translateX(-50%)',
          pointerEvents: 'none',
        }}
      >
        <div
          style={{
            display: 'inline-flex',
            alignItems: 'center',
            gap: 5,
            padding: '3px 10px',
            borderRadius: 999,
            fontSize: 10.5,
            fontWeight: 600,
            color: 'var(--ol-blue)',
            background: 'var(--ol-capsule-badge-bg)',
            // issue #470：去掉无效的 backdrop-filter —— webview 模糊不了透明窗口背后的桌面
            // （Tauri 上游限制，同本文件上方 pill 注释），纯空耗合成，删除零视觉变化。
            border: '0.5px solid var(--ol-capsule-badge-border)',
            boxShadow: '0 4px 12px -4px rgba(37, 99, 235, 0.25), 0 0 0 0.5px rgba(0,0,0,0.04)',
            letterSpacing: '0.02em',
            whiteSpace: 'nowrap',
            // 隐藏：从 pill 中线偏下出发；显示：归位到 wrapper（pill 上方 25px）
            opacity: translation ? 1 : 0,
            transform: translation ? 'translateY(0) scale(1)' : 'translateY(40px) scale(.88)',
            transformOrigin: 'center bottom',
            transition: 'opacity .24s ease-out, transform .34s cubic-bezier(.2,.9,.3,1.1)',
            willChange: 'opacity, transform',
          }}
        >
          <span style={{ width: 5, height: 5, borderRadius: 999, background: 'var(--ol-blue)' }} />
          {t('capsule.translating')}
        </div>
      </div>
      <Pill
        os={os}
        state={renderedState}
        level={leaving ? 0 : level}
        insertedChars={insertedChars}
        message={message}
        operating={operating}
        onCancel={onCancel}
        onConfirm={onConfirm}
      />
    </div>
  );
}
