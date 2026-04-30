// Capsule.tsx — 1:1 移植 `Sources/OpenLessUI/CapsuleView.swift`。
//
// Swift 原版用的是 macOS 的 `.ultraThinMaterial` + 白色描边（macOS 26 用 Liquid Glass），
// **不是**深色 pill —— design_handoff_openless/capsule.jsx 那个 dark pill 是早期设计稿，
// Swift 实际产品迁到了系统磨砂材质上。
//
// 视觉规格（与 Swift 同步）：
//   - 总尺寸 176×42 pill
//   - 浅色磨砂背景 (white 0.62 alpha + backdrop blur 28px) + 白色 1px 边框 (alpha 0.34)
//   - 左/右 28×28 圆形按钮：cancel 用半透明 thinMaterial 风、confirm 用 white 0.92
//   - 中间 84pt 宽 slot：根据状态切换 audio bars / dots+text / 状态文字
//
// 状态语义对齐 Swift CapsuleState：
//   listening   → 5 根 audio bars [.55,.85,1,.85,.55] base 4pt + level*14pt
//   processing  → 3 个跳动的圆点 + "正在思考中"
//   inserted    → "已插入"
//   cancelled   → "已取消"
//   copied      → "已复制 ⌘V"
//   error(msg)  → 红字
//
// 控件可用性：仅 listening 时 cancel/confirm 才能点（与 Swift `isControlEnabled` 一致）。

import { useEffect, useState } from 'react';
import { invokeOrMock, isTauri } from '../lib/ipc';
import type { CapsulePayload, CapsuleState } from '../lib/types';

interface AudioBarsProps {
  level: number;
}

/// 5 根 envelope 条；level=0 时全收到 4pt 基线 → 视觉静止；level↑ → 中间最高条往上拔。
function AudioBars({ level }: AudioBarsProps) {
  const envelope = [0.55, 0.85, 1.0, 0.85, 0.55];
  const base = 4;
  const max = 18;
  const voice = Math.min(1, Math.max(0, level));
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
            height: base + (max - base) * voice * env,
            borderRadius: 999,
            background: 'var(--ol-blue)',
            opacity: 0.82,
            // Swift spring(response: 0.18, damping: 0.7) 的 cubic-bezier 近似
            transition: 'height 0.18s cubic-bezier(.5, 1.7, .5, 1)',
          }}
        />
      ))}
    </div>
  );
}

/// 3 个圆点错相位脉动；总宽 20pt，与 Swift ProgressDots 一致。
function ProcessingDots() {
  return (
    <div style={{ display: 'inline-flex', alignItems: 'center', gap: 4, width: 20 }}>
      {[0, 1, 2].map(i => (
        <span
          key={i}
          style={{
            width: 4,
            height: 4,
            borderRadius: 999,
            background: 'var(--ol-blue)',
            opacity: 0.85,
            animation: `cap-dot 0.9s linear ${i * 0.3}s infinite`,
          }}
        />
      ))}
    </div>
  );
}

interface CenterTextProps {
  text: string;
  color?: string;
}

function CenterText({ text, color = 'var(--ol-ink-3)' }: CenterTextProps) {
  return (
    <span
      style={{
        fontSize: 11,
        fontWeight: 500,
        color,
        width: 84,
        textAlign: 'center',
        lineHeight: 1,
        whiteSpace: 'nowrap',
        overflow: 'hidden',
        textOverflow: 'ellipsis',
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

function CircleButton({ variant, enabled, onClick }: CircleButtonProps) {
  const isCancel = variant === 'cancel';
  return (
    <button
      onClick={enabled ? onClick : undefined}
      aria-label={isCancel ? 'cancel' : 'confirm'}
      disabled={!enabled}
      style={{
        width: 28,
        height: 28,
        borderRadius: 999,
        // Swift cancel: .thinMaterial; confirm: white(0.92)
        background: isCancel ? 'rgba(255, 255, 255, 0.55)' : 'rgba(255, 255, 255, 0.92)',
        backdropFilter: isCancel ? 'blur(12px) saturate(160%)' : 'none',
        WebkitBackdropFilter: isCancel ? 'blur(12px) saturate(160%)' : 'none',
        color: 'var(--ol-ink)',
        border: '0.8px solid rgba(0, 0, 0, 0.08)',
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'center',
        cursor: enabled ? 'default' : 'not-allowed',
        opacity: enabled ? 1 : 0.42,
        flexShrink: 0,
        padding: 0,
        boxShadow: '0 1px 2px rgba(0, 0, 0, 0.06)',
      }}
    >
      {isCancel ? (
        // SF Symbol "xmark" 等价
        <svg width="11" height="11" viewBox="0 0 11 11">
          <path d="M1.5 1.5l8 8M9.5 1.5l-8 8" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
        </svg>
      ) : (
        // SF Symbol "checkmark" 等价
        <svg width="13" height="13" viewBox="0 0 13 13">
          <path d="M2 6.5l3.2 3.5L11 3.5" stroke="currentColor" strokeWidth="1.7" fill="none" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      )}
    </button>
  );
}

interface PillProps {
  state: CapsuleState;
  level: number;
  insertedChars: number;
  message?: string;
  onCancel: () => void;
  onConfirm: () => void;
}

function Pill({ state, level, insertedChars, message, onCancel, onConfirm }: PillProps) {
  // 与 Swift `isControlEnabled` 同语义：只有 listening 时 cancel/confirm 才可点。
  const enabled = state === 'recording';

  let center: JSX.Element;
  switch (state) {
    case 'recording':
      center = <AudioBars level={level} />;
      break;
    case 'transcribing':
    case 'polishing':
      center = (
        <div style={{ display: 'inline-flex', alignItems: 'center', gap: 6, width: 84, justifyContent: 'center' }}>
          <ProcessingDots />
          <span style={{ fontSize: 10.5, fontWeight: 500, color: 'var(--ol-ink-2)' }}>
            正在思考中
          </span>
        </div>
      );
      break;
    case 'done':
      center = <CenterText text={message || `已插入 ${insertedChars}`} />;
      break;
    case 'cancelled':
      center = <CenterText text="已取消" />;
      break;
    case 'error':
      center = <CenterText text={message || '出错了'} color="var(--ol-err)" />;
      break;
    default:
      center = <AudioBars level={0} />;
  }

  return (
    <div
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        gap: 8,
        padding: '0 8px',
        width: 176,
        height: 42,
        borderRadius: 999,
        // Swift `.ultraThinMaterial` + InputBarChrome 的浅色磨砂效果
        background: 'rgba(255, 255, 255, 0.62)',
        backdropFilter: 'blur(28px) saturate(180%)',
        WebkitBackdropFilter: 'blur(28px) saturate(180%)',
        border: '1px solid rgba(255, 255, 255, 0.55)',
        boxShadow:
          '0 18px 50px -10px rgba(0, 0, 0, 0.20),' +
          ' 0 0 0 0.5px rgba(0, 0, 0, 0.08),' +
          ' inset 0 0.5px 0 rgba(255, 255, 255, 0.55)',
        color: 'var(--ol-ink)',
        fontFamily: 'var(--ol-font-sans)',
      }}
    >
      <CircleButton variant="cancel" enabled={enabled} onClick={onCancel} />
      <div style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
        {center}
      </div>
      <CircleButton variant="confirm" enabled={enabled} onClick={onConfirm} />
    </div>
  );
}

export function Capsule() {
  // 浏览器 dev 默认显示 listening；Tauri 进来后由后端 idle 覆盖。
  const [state, setState] = useState<CapsuleState>(isTauri ? 'idle' : 'recording');
  const [level, setLevel] = useState<number>(isTauri ? 0 : 0.6);
  const [insertedChars, setInsertedChars] = useState<number>(0);
  const [message, setMessage] = useState<string | undefined>();

  useEffect(() => {
    if (!isTauri) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    (async () => {
      const { listen } = await import('@tauri-apps/api/event');
      const handle = await listen<CapsulePayload>('capsule:state', event => {
        const p = event.payload;
        setState(p.state);
        setLevel(p.level ?? 0);
        setMessage(p.message ?? undefined);
        if (p.insertedChars != null) setInsertedChars(p.insertedChars);
      });
      if (cancelled) handle();
      else unlisten = handle;
    })();
    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, []);

  const onCancel = () => {
    void invokeOrMock<void>('cancel_dictation', undefined, () => undefined);
  };
  const onConfirm = () => {
    void invokeOrMock<void>('stop_dictation', undefined, () => undefined);
  };

  // idle 状态视觉上隐藏（panel 也会被后端 hide）；保留容器避免 React 卸载抖动。
  if (state === 'idle') {
    return <div style={{ width: 0, height: 0 }} />;
  }

  return (
    <div
      style={{
        width: '100%',
        height: '100%',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        background: 'transparent',
        animation: 'capsule-in .22s cubic-bezier(.2,.9,.3,1.1)',
      }}
    >
      <Pill
        state={state}
        level={level}
        insertedChars={insertedChars}
        message={message}
        onCancel={onCancel}
        onConfirm={onConfirm}
      />
      <style>{`
        @keyframes capsule-in {
          from { opacity: 0; transform: translateY(6px) scale(.96); }
          to   { opacity: 1; transform: translateY(0) scale(1); }
        }
        @keyframes cap-dot {
          0%, 100% { opacity: 0.3; transform: scale(0.8); }
          50%      { opacity: 1.0; transform: scale(1.0); }
        }
      `}</style>
    </div>
  );
}
