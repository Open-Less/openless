import { AnimatePresence, motion } from 'framer-motion';
import type { CSSProperties } from 'react';
import { useEffect, useState } from 'react';

export type SaveToastState = 'idle' | 'saving' | 'saved' | 'failed';

// 弹框入场 / 退场方向 —— 始终"从哪来，回哪去"（同一方向进出）。
//   'right'：从屏幕右侧滑入、再滑回右侧 —— 页面级 toast（风格包 / 翻译 / 划词…）。
//   'top'  ：从屏幕上方滑入、再滑回上方 —— 设置弹窗内的 toast。
export type ToastSlideFrom = 'right' | 'top';

interface SavedToastProps {
  saveState: SaveToastState;
  message: string;
  offsetStyle?: Pick<CSSProperties, 'top' | 'right' | 'left' | 'bottom' | 'position'>;
  slideFrom?: ToastSlideFrom;
}

export function SavedToast({ saveState, message, offsetStyle, slideFrom = 'right' }: SavedToastProps) {
  // 维护内部状态，使通知可以自己倒计时关闭（即使用户父组件的 timer 长于 0.8s）
  const [internalVisible, setInternalVisible] = useState(false);

  useEffect(() => {
    if (saveState !== 'idle') {
      setInternalVisible(true);
      // 满足用户要求：弹出后约 0.8 秒自动收回
      const timer = window.setTimeout(() => setInternalVisible(false), 800);
      return () => window.clearTimeout(timer);
    }
    setInternalVisible(false);
  }, [saveState, message]);

  const failed = saveState === 'failed';

  // 统一停靠右上角 —— 跟「风格市场 / 刷新 / 导入 ZIP」这排页头按钮同区。
  // position:fixed 锚视口：滑入 / 滑出都贴着屏幕边走，不会在页面里撑出滚动条。
  // 设置弹窗自行传 offsetStyle 覆盖成 absolute（锚到弹窗内容区右上角）。
  const style: CSSProperties = {
    position: 'fixed',
    top: 20,
    right: 28,
    ...offsetStyle,
    zIndex: 99999,
    padding: '4px 11px',
    borderRadius: 999,
    border: failed
      ? '0.5px solid rgba(239,68,68,0.22)'
      : '0.5px solid rgba(37,99,235,0.16)',
    background: failed ? 'rgba(254,242,242,0.92)' : 'rgba(239,244,255,0.92)',
    color: failed ? '#dc2626' : '#2563eb',
    fontSize: 11.5,
    fontWeight: 600,
    lineHeight: 1.5,
    boxShadow: failed
      ? '0 4px 12px -8px rgba(239,68,68,.28)'
      : '0 4px 12px -8px rgba(37,99,235,.26)',
    backdropFilter: 'blur(12px) saturate(160%)',
    WebkitBackdropFilter: 'blur(12px) saturate(160%)',
    pointerEvents: 'none',
    whiteSpace: 'nowrap',
    display: 'flex',
    alignItems: 'center',
    gap: 6,
  };

  // "从哪来，回哪去"：入场起点 == 退场终点，方向由 slideFrom 决定。
  // 两个分支都写全 x / y —— motion variant 保持完整，避免另一轴落到隐式默认值。
  const offscreen = slideFrom === 'top'
    ? { opacity: 0, x: 0, y: '-220%' }
    : { opacity: 0, x: '120%', y: 0 };

  return (
    <AnimatePresence>
      {internalVisible && (
        <motion.div
          role={failed ? 'alert' : 'status'}
          initial={{ ...offscreen, filter: 'blur(8px)' }}
          animate={{ opacity: 1, x: 0, y: 0, filter: 'blur(0px)' }}
          exit={{ ...offscreen, filter: 'blur(4px)' }}
          transition={{ type: 'spring', damping: 20, stiffness: 260 }}
          style={style}
        >
          {failed ? '⚠️' : '✓'} {message}
        </motion.div>
      )}
    </AnimatePresence>
  );
}
