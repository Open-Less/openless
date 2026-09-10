import { Icon } from './Icon';
import { memo, useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { detectOS, type OS } from './WindowChrome';
import { warmUpSiriShaders } from './SiriGL';
import { VoiceOrbStage } from './VoiceOrbStage';
import { TypelessCapsule } from './TypelessCapsule';
import { getSettings } from '../lib/ipc/settings';
import { cancelDictation, resumeCancelledRecording, stopDictation } from '../lib/ipc/dictation';
import {
  getCapsuleHostMetrics,
  getCapsuleMessageLayout,
  getCapsulePillMetrics,
  parseCapsuleStyle,
} from '../lib/capsuleLayout';
import { isTauri } from '../lib/ipc';
import type {
  CapsulePayload,
  CapsuleState,
  CapsuleStyle,
  InsertFallbackCardPayload,
  PendingCorrection,
} from '../lib/types';
import { VocabSuggestionCard } from './VocabSuggestionCard';
import { InsertFallbackCard } from './InsertFallbackCard';
import {
  applyTranscriptEvent,
  type BackendEvent,
  type TranscriptViewState,
} from '../lib/backendEvent';

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
  /* 经典药丸（Openless 默认风格）专属：
     - cap-shine：转写/润色中"thinking" 文字的蓝光扫过；
     - cap-state-enter：状态内容切换时的淡入。 */
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
 * 选区润色专用的一行无交互提示。它处在同一扇 Windows no-activate + mouse-passthrough
 * capsule 窗口中，因此不会把焦点从用户当前输入框抢走，也不会遮挡点击。
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

function RecordingRecoveryPrompt({ sessionId, os }: { sessionId: string; os: OS }) {
  const { t } = useTranslation();
  const [resuming, setResuming] = useState(false);
  const [failed, setFailed] = useState(false);

  const resume = async () => {
    if (resuming) return;
    setResuming(true);
    setFailed(false);
    try {
      await resumeCancelledRecording(sessionId);
    } catch (error) {
      console.error('[capsule] resume cancelled recording failed', error);
      setFailed(true);
      setResuming(false);
    }
  };

  const continueLabel = t('capsule.recovery.continue');
  const failedLabel = t('capsule.recovery.failed');

  return (
    <button
      type="button"
      className="ol-frost ol-capsule-pill"
      aria-live="polite"
      aria-busy={resuming}
      aria-label={failed ? `${continueLabel}. ${failedLabel}` : undefined}
      title={failed ? failedLabel : undefined}
      onClick={() => void resume()}
      onMouseDown={(event) => {
        event.preventDefault();
        event.stopPropagation();
      }}
      disabled={resuming}
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'center',
        minWidth: os === 'win' ? 160 : 144,
        maxWidth: os === 'win' ? 216 : 196,
        height: os === 'win' ? 52 : 42,
        padding: os === 'win' ? '0 22px' : '0 18px',
        borderRadius: 999,
        background: 'var(--ol-capsule-pill-bg)',
        border: '1px solid var(--ol-capsule-pill-border)',
        boxShadow: 'var(--ol-capsule-pill-shadow), var(--ol-capsule-pill-inset)',
        color: failed ? 'var(--ol-err)' : 'var(--ol-blue)',
        fontFamily: 'var(--ol-font-sans)',
        fontSize: 11.5,
        fontWeight: 600,
        lineHeight: 1,
        letterSpacing: '0.005em',
        whiteSpace: 'nowrap',
        pointerEvents: 'auto',
        boxSizing: 'border-box',
        appearance: 'none',
        cursor: 'default',
        opacity: resuming ? 0.46 : 0.92,
      }}
    >
      {resuming ? t('capsule.recovery.resuming') : continueLabel}
    </button>
  );
}

// ───────── 经典药丸（Openless 默认风格）───────────────────────────────
// 从 1.3.14 的胶囊实现恢复：毛玻璃药丸 + 音量条 + 取消/确认按钮。尺寸与当时
// capsuleLayout 的常量保持一致（Siri 光效舞台是 460×180 全幅，经典版是居中小药丸）。

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

function AudioBars({ level }: { level: number }) {
  const envelope = [0.55, 0.85, 1.0, 0.85, 0.55];
  const base = 2;
  const max = 24;
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
  operating?: boolean;
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
  operating?: boolean;
  translation: boolean;
}

/**
 * 经典药丸 + 「正在翻译」徽章（与 1.3.14 一致）。取消/确认按钮直接调 dictation
 * 命令（胶囊窗口本身不抢焦点，按钮交互只出现在录音进行中）。
 */
function ClassicCapsule({
  os,
  state,
  level,
  insertedChars,
  message,
  operating,
  translation,
}: ClassicCapsuleProps) {
  const { t } = useTranslation();
  const metrics = classicPillMetrics(os);
  const hostMetrics = getCapsuleHostMetrics(os, false, 'classic');
  const onCancel = useCallback(() => {
    void cancelDictation();
  }, []);
  const onConfirm = useCallback(() => {
    void stopDictation();
  }, []);

  return (
    <>
      {/* "正在翻译" 徽章 — 嵌套两层：
          外层只负责"绝对定位 + 水平居中（translateX(-50%)）"，不参与动画；
          内层只负责"垂直位移 + 渐变透明度"——这样不会跟 translateX(-50%) 冲突，
          也不存在 keyframe 与 inline transform 互相覆盖导致的视觉跳变。 */}
      <div
        style={{
          position: 'absolute',
          left: '50%',
          bottom: hostMetrics.bottomInset + metrics.height + hostMetrics.badgeGap,
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
            // （Tauri 上游限制，同 pill 注释），纯空耗合成，删除零视觉变化。
            border: '0.5px solid var(--ol-capsule-badge-border)',
            boxShadow: '0 4px 12px -4px rgba(37, 99, 235, 0.25), 0 0 0 0.5px rgba(0,0,0,0.04)',
            letterSpacing: '0.02em',
            whiteSpace: 'nowrap',
            // 隐藏：从 pill 中线偏下出发；显示：归位到 pill 上方。
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
    </>
  );
}

// 与 @keyframes capsule-out 的时长一致——必须同步，否则定时器先于动画结束就 unmount →
// 用户看到半截动画被截断。两种样式沿用同一组 capsule-in/out keyframes，只是时长不同：
// Siri 光效 520ms（现状），经典药丸 360ms（与 1.3.14 一致）。
const EXIT_ANIM_MS_SIRI = 520;
const EXIT_ANIM_MS_CLASSIC = 360;

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
      recoverySessionId: null,
      style: 'siri' as CapsuleStyle,
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
    recoverySessionId: params.get('recoverySessionId'),
    style: parseCapsuleStyle(params.get('style')) ?? 'siri',
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
  const transcriptViewRef = useRef<TranscriptViewState>({
    sessionId: null,
    sequence: 0,
    text: '',
  });
  const [translation, setTranslation] = useState<boolean>(preview.translation);
  const [selectionPolish, setSelectionPolish] = useState<boolean>(preview.selectionPolish);
  const [recoverySessionId, setRecoverySessionId] = useState<string | null>(
    preview.recoverySessionId,
  );
  // 偏好事件即时换肤；录音状态携带同一个样式，保证首次显示也能正确呈现。
  const [capsuleStyle, setCapsuleStyle] = useState<CapsuleStyle>(preview.style);
  const stylePreferenceReadyRef = useRef(false);
  const isClassic = capsuleStyle === 'classic';
  const isTypeless = capsuleStyle === 'typeless';
  // 预备态：麦克风尚未吐第一帧 PCM。true 时录音光条走「待命」呼吸形态（见 SiriGL warming）。
  const [warming, setWarming] = useState<boolean>(preview.warming);
  // 预备→就绪耗时的移动平均，驱动光条展开动画的预测节奏（见 SiriGL warmProgress）。
  const [warmupMs, setWarmupMs] = useState<number>(() => readWarmupMs());
  const warmStartRef = useRef<number | null>(null);
  // 经典药丸专属：done 态的「已插入 N 字」与 thinking/using 文案。payload 只在状态变化
  // 时到达，用 ref 保存即可 —— 触发重渲的是 setState，这里只是给渲染读取终态值。
  const insertedCharsRef = useRef(0);
  const operatingRef = useRef(false);
  // `leaving` 与 `lastVisibleState` 协同实现「退出动画」：
  // - 当 state 从非 idle 变成 idle 时，不立即卸载，而是把 leaving 置为 true 并保留
  //   最后一帧的可见 state（lastVisibleState），让胶囊用 capsule-out 动画收缩淡出。
  // - 动画结束（EXIT_ANIM_MS）后再把 leaving 置回 false，组件回到「真正未挂载」分支。
  // - 若期间 state 又切回非 idle（例如用户连按热键），立刻中止 leaving 并恢复显示。
  const [leaving, setLeaving] = useState<boolean>(false);
  const [lastVisibleState, setLastVisibleState] = useState<CapsuleState>(preview.state);
  const [lastVisibleSelectionPolish, setLastVisibleSelectionPolish] = useState<boolean>(
    preview.selectionPolish,
  );
  const [lastVisibleMessage, setLastVisibleMessage] = useState<string | undefined>(preview.message);
  const [lastVisibleRecoverySessionId, setLastVisibleRecoverySessionId] = useState<string | null>(
    preview.recoverySessionId,
  );
  // 退出动画时长跟随样式；leaving effect 故意只依赖 state，所以经 ref 读取最新值。
  const exitMsRef = useRef(capsuleStyle === 'siri' ? EXIT_ANIM_MS_SIRI : EXIT_ANIM_MS_CLASSIC);
  exitMsRef.current = capsuleStyle === 'siri' ? EXIT_ANIM_MS_SIRI : EXIT_ANIM_MS_CLASSIC;
  const exitMs = exitMsRef.current;
  // 词条建议卡片。走独立事件通道，不进会话状态机 —— 那套状态机身上挂着 Esc 独占、
  // Space 贴附、多屏定位一整串逻辑，加一个非会话状态进去只会污染它。
  const [suggestions, setSuggestions] = useState<PendingCorrection[]>([]);
  // 落字失败兜底卡片。与词条卡片同一套路：独立事件通道，不进会话状态机。
  const [insertFallback, setInsertFallback] = useState<InsertFallbackCardPayload | null>(null);
  const hostMetrics = getCapsuleHostMetrics(os, translation, capsuleStyle);
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
      const handle = await listen<CapsulePayload>('capsule:state', (event) => {
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
        if (p.state === 'recording') setLocalAsrText('');
        setTranslation(p.translation === true);
        setWarming(p.warming === true);
        setSelectionPolish(p.selectionPolish === true);
        setRecoverySessionId(p.recoverySessionId ?? null);
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

  // 先订阅再读取偏好，兼顾隐藏窗口首次加载和已经播放中的即时换肤。
  useEffect(() => {
    if (!isTauri) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    let preferenceRevision = 0;
    (async () => {
      const { listen } = await import('@tauri-apps/api/event');
      const handle = await listen<{ capsuleStyle?: CapsuleStyle }>('prefs:changed', (event) => {
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
        stylePreferenceReadyRef.current = true;
        setCapsuleStyle(next);
      }
    })().catch((error) => console.warn('[capsule] preferences subscription failed', error));
    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, []);

  // 退出动画调度：在 state 真正进入 idle 时，先用 capsule-out 播放
  // EXIT_ANIM_MS_SIRI / EXIT_ANIM_MS_CLASSIC（按当前样式，经 exitMsRef 读取），再卸载。
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
    }, exitMsRef.current);
    return () => clearTimeout(timer);
    // 故意只依赖 state —— lastVisibleState / leaving 是内部派生量，
    // 把它们加进依赖会让定时器被反复重建。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state]);

  // Idle payload 不携带终态文案。保留最近一帧可见 selection 提示，才能让 auto-hide
  // 触发后的离场动画继续显示“已替换 / 未选中内容”等正确反馈，而不闪回语音舞台。
  useEffect(() => {
    if (state === 'idle') return;
    setLastVisibleSelectionPolish(selectionPolish);
    setLastVisibleMessage(message);
    setLastVisibleRecoverySessionId(recoverySessionId);
  }, [state, selectionPolish, message, recoverySessionId]);

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

  // 兜底卡片排在最前：它是在会话收尾那一刻弹的，那一帧胶囊还在渲染 Done/Error 终态，
  // 而这次会话的结果恰恰是「没落进去」—— 让终态盖在上面等于报了个假的成功。
  // 后端那边同步让路：卡片可见时 idle 隐藏不收窗口（见 capsule_focus.rs）。
  if (insertFallback) {
    return <InsertFallbackCard payload={insertFallback} />;
  }

  // 卡片优先：它和胶囊共用一个窗口，但时序上不冲突 —— 卡片在改完字之后才弹，那时
  // 会话早已收尾；新一轮听写开始时后端会先把卡片收起来（见 begin_session_as）。
  if (suggestions.length > 0) {
    return <VocabSuggestionCard suggestions={suggestions} />;
  }

  // 真正卸载：state 已是 idle，且不在离场动画中。
  if (state === 'idle' && !leaving) {
    return <div style={{ width: 0, height: 0 }} />;
  }

  // 离场时用 lastVisibleState 渲染最后一帧内容，避免把 idle 当作无波形状态。
  const renderedState: CapsuleState = state === 'idle' ? lastVisibleState : state;
  const renderedSelectionPolish = state === 'idle' ? lastVisibleSelectionPolish : selectionPolish;
  const renderedMessage =
    state === 'idle'
      ? lastVisibleMessage
      : state === 'transcribing' && localAsrText
        ? localAsrText
        : message;
  const renderedRecoverySessionId =
    state === 'idle' ? lastVisibleRecoverySessionId : recoverySessionId;

  return (
    <div
      style={{
        width: '100%',
        height: '100%',
        position: 'relative',
        display: 'flex',
        alignItems: capsuleStyle === 'siri' ? 'center' : 'flex-end',
        justifyContent: 'center',
        paddingLeft: hostMetrics.horizontalInset,
        paddingRight: hostMetrics.horizontalInset,
        paddingBottom: hostMetrics.bottomInset,
        boxSizing: hostMetrics.boxSizing,
        background: 'transparent',
        animation: leaving
          ? `capsule-out ${exitMs}ms cubic-bezier(.55,.06,.68,.19) forwards`
          : // .68s 的入场被反馈「按下之后有延迟」：压到 .38s，曲线保持轻微弹性，
            // 光条几乎跟手出现。
            'capsule-in .38s cubic-bezier(.3,1.2,.4,1) both',
        transformOrigin: 'center',
        willChange: 'transform, opacity',
      }}
    >
      {renderedRecoverySessionId ? (
        <RecordingRecoveryPrompt sessionId={renderedRecoverySessionId} os={os} />
      ) : (
        !renderedSelectionPolish &&
        (isClassic ? (
          <ClassicCapsule
            os={os}
            state={renderedState}
            level={leaving ? 0 : level}
            insertedChars={insertedCharsRef.current}
            message={renderedMessage}
            operating={operatingRef.current}
            translation={translation}
          />
        ) : isTypeless ? (
          <TypelessCapsule
            state={renderedState}
            level={leaving ? 0 : level}
            insertedChars={insertedCharsRef.current}
            message={renderedMessage}
            operating={operatingRef.current}
            translation={translation}
            warming={!leaving && warming}
          />
        ) : (
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
                  color: 'var(--ol-capsule-center-ink)',
                  background: 'var(--ol-capsule-badge-bg)',
                  border: '1px solid var(--ol-capsule-pill-border)',
                  borderRadius: 999,
                  padding: '4px 10px',
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
        ))
      )}
      {!renderedRecoverySessionId && renderedSelectionPolish && (
        <SelectionPolishNotice state={renderedState} message={renderedMessage} />
      )}
    </div>
  );
}
