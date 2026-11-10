// 通用 HTTP 桥：任何 webview（Qt WebEngine / CEF / 普通浏览器）都能用。
//
// 与 `gtkBridge.ts` 的区别只在传输层：那边走 WebKit 的 message handler，
// 这边就是 `fetch` + SSE，后端是同一个 Rust 进程（`/api/invoke`、`/api/events`）。
// 命令面、参数形状、返回值约定与 Tauri/GTK 完全一致，所以页面代码不用改。
export const isHttpBridge =
  typeof window !== 'undefined' &&
  new URLSearchParams(window.location.search).get('bridge') === 'http';

type Pending = { resolve: (value: string) => void; reject: (error: Error) => void };
const pending = new Map<string, Pending>();
let nextId = 1;
let eventsAttached = false;

function attachEvents() {
  if (eventsAttached || typeof EventSource === 'undefined') return;
  eventsAttached = true;
  const source = new EventSource('/api/events');
  source.onmessage = (message) => {
    try {
      const { name, payload } = JSON.parse(message.data) as { name: string; payload: string };
      (window as unknown as { __openlessEvent?: (n: string, p: string) => void }).__openlessEvent?.(
        name,
        payload,
      );
    } catch {
      /* 心跳或解析失败都不影响后续事件 */
    }
  };
  // EventSource 自带重连；这里只记录，避免刷屏。
  source.onerror = () => {};
}

export async function invokeHttp<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  attachEvents();
  const id = `r${nextId++}`;
  const wire = await new Promise<string>((resolve, reject) => {
    pending.set(id, { resolve, reject });
    fetch('/api/invoke', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ id, command, args: args ?? {} }),
    })
      .then((response) => response.json())
      .then((reply: { ok: boolean; value?: unknown; error?: string }) => {
        pending.delete(id);
        if (!reply.ok) {
          reject(new Error(reply.error ?? '命令失败'));
          return;
        }
        resolve(JSON.stringify(reply.value ?? null));
      })
      .catch((error: Error) => {
        pending.delete(id);
        reject(error);
      });
  });
  const parsed = JSON.parse(wire) as T;
  if (parsed === undefined) throw new Error(`命令 ${command} 没有返回`);
  return parsed;
}
