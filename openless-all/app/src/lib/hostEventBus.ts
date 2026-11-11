// 宿主事件总线：把「原生推来的事件」统一成前端可订阅的形式。
//
// 三种宿主都要覆盖：
//   Tauri   → `@tauri-apps/api/event` 的 listen()
//   GTK/HTTP→ 宿主注入的 `window.__openlessEvent(name, payloadJson)`
// 以前只有 Tauri 那条能用，另外两条（我们自己的 Linux 宿主）事件全被丢掉，
// 所以云同步这类「靠事件驱动」的界面永远不刷新。

import { isGtkHost } from './gtkBridge';
import { isHttpBridge } from './httpBridge';

type Handler = (payload: unknown) => void;

const listeners = new Map<string, Set<Handler>>();

declare global {
  interface Window {
    __openlessEvent?: (name: string, payloadJson: string) => void;
  }
}

/** 宿主侧调用：把事件派发给所有订阅者。 */
export function emitHostEvent(name: string, payloadJson: string): void {
  const handlers = listeners.get(name);
  if (!handlers || handlers.size === 0) return;
  let payload: unknown = null;
  try {
    payload = JSON.parse(payloadJson);
  } catch {
    payload = payloadJson;
  }
  for (const handler of [...handlers]) {
    try {
      handler(payload);
    } catch (error) {
      console.warn('[host-event] handler failed', name, error);
    }
  }
}

// 原生宿主（GTK 的 evaluate_javascript / HTTP 的 SSE）都通过这个全局函数投递。
if (typeof window !== 'undefined') {
  window.__openlessEvent = emitHostEvent;
}

export const isNativeHost = isGtkHost || isHttpBridge;

/**
 * 订阅一个宿主事件。返回取消订阅函数。
 * 非原生宿主（纯浏览器预览）返回空操作，不会抛错。
 */
export function listenHostEvent(name: string, handler: Handler): () => void {
  if (!isNativeHost) return () => {};
  let set = listeners.get(name);
  if (!set) {
    set = new Set();
    listeners.set(name, set);
  }
  set.add(handler);
  return () => {
    set?.delete(handler);
  };
}
