// 本机 GTK4 + WebKitGTK 宿主的桥（不是 Tauri、不是鸿蒙）。
//
// 协议形状与 `harmonyBridge.ts` 一致，只是传输换成 WebKit 的 script message handler：
//   JS → native : window.webkit.messageHandlers.openless.postMessage({ id, command, args })
//   native → JS : window.__openlessResolve(id, '{"ok":true,"value":…}')
//
// 原生侧把 args 当 JSON 字符串收，返回也是 JSON 字符串；这样命令面不需要为每种
// 参数类型生成绑定，和鸿蒙那条完全同构。
export const isGtkHost = import.meta.env?.VITE_OPENLESS_PLATFORM === 'gtk';

declare global {
  interface Window {
    webkit?: {
      messageHandlers?: Record<string, { postMessage: (payload: unknown) => void }>;
    };
    __openlessResolve?: (id: string, wire: string) => void;
  }
}

type Pending = { resolve: (value: string) => void; reject: (error: Error) => void };
const pending = new Map<string, Pending>();
let nextId = 1;

if (isGtkHost && typeof window !== 'undefined') {
  window.__openlessResolve = (id: string, wire: string) => {
    const entry = pending.get(id);
    if (!entry) return;
    pending.delete(id);
    entry.resolve(wire);
  };
}

export async function invokeGtk<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const handler = window.webkit?.messageHandlers?.openless;
  if (!handler) throw new Error('GTK 宿主桥尚未就绪');
  const id = `r${nextId++}`;
  const wire = await new Promise<string>((resolve, reject) => {
    pending.set(id, { resolve, reject });
    window.setTimeout(() => {
      if (pending.delete(id)) reject(new Error(`GTK 命令超时：${command}`));
    }, 120_000);
    // WebKit 的 script message handler 只能可靠地取到字符串，所以整条消息
    // 先 JSON.stringify 再发；原生侧再解析一次。
    handler.postMessage(JSON.stringify({ id, command, args: JSON.stringify(args ?? {}) }));
  });
  const result = JSON.parse(wire) as { ok: boolean; value?: T; error?: string };
  if (!result.ok) throw new Error(result.error ?? `GTK 命令失败：${command}`);
  return result.value as T;
}
