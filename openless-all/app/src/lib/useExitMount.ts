// 关闭浮层时保持挂载至退出动画结束；再次打开会取消待卸载计时器。
// 调用方用 mounted 控制渲染、closing 控制退场样式，exitMs 与 CSS 动画时长保持一致。

import { useEffect, useState } from 'react';

export function useExitMount(open: boolean, exitMs = 200) {
  const [mounted, setMounted] = useState(open);
  const [closing, setClosing] = useState(false);
  useEffect(() => {
    if (open) {
      setMounted(true);
      setClosing(false);
      return;
    }
    if (!mounted) return;
    setClosing(true);
    const timer = window.setTimeout(() => {
      setMounted(false);
      setClosing(false);
    }, exitMs);
    return () => window.clearTimeout(timer);
  }, [open, mounted, exitMs]);
  return { mounted, closing };
}
