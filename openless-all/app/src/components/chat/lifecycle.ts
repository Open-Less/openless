// lifecycle.ts — 聊天面板（qa / less-computer）的出现/消失动画信号。
//
// 浮窗是常驻 webview（hide/show 复用，不重挂载），CSS 入场动画只在首次 mount
// 播一次——之后每次唤起都是「突然出现」。后端在 show/hide 路径发生命周期事件：
//   · `chat-panel:shown`   → enterEpoch+1（作 key 重放 olchat-shell-in 入场）
//   · `chat-panel:closing` → closing=true（加 olchat-shell-out 播退场，240ms 后
//     后端才真正 hide；期间再次 shown 会复位）
// 浏览器预览（非 Tauri）没有这些事件，保持首次 mount 的入场动画即可。

import { useEffect, useState } from 'react';
import { isTauri } from '../../lib/ipc/shared';

export function useChatPanelLifecycle(): { enterEpoch: number; closing: boolean } {
  const [enterEpoch, setEnterEpoch] = useState(0);
  const [closing, setClosing] = useState(false);

  useEffect(() => {
    if (!isTauri) return;
    let unShown: (() => void) | undefined;
    let unClosing: (() => void) | undefined;
    let cancelled = false;
    (async () => {
      try {
        const { listen } = await import('@tauri-apps/api/event');
        const shownHandle = await listen('chat-panel:shown', () => {
          setClosing(false);
          setEnterEpoch(epoch => epoch + 1);
        });
        const closingHandle = await listen('chat-panel:closing', () => {
          setClosing(true);
        });
        if (cancelled) {
          shownHandle();
          closingHandle();
        } else {
          unShown = shownHandle;
          unClosing = closingHandle;
        }
      } catch (error) {
        console.error('[chat-panel] lifecycle listener setup failed', error);
      }
    })();
    return () => {
      cancelled = true;
      unShown?.();
      unClosing?.();
    };
  }, []);

  return { enterEpoch, closing };
}
