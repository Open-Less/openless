// 本机 GTK4 + WebKitGTK 宿主的桥（不是 Tauri、不是鸿蒙）。
//
// 协议形状与 `harmonyBridge.ts` 一致，只是传输换成 WebKit 的 script message handler：
//   JS → native : window.webkit.messageHandlers.openless.postMessage({ id, command, args })
//   native → JS : window.__openlessResolve(id, '{"ok":true,"value":…}')
//
// 原生侧把 args 当 JSON 字符串收，返回也是 JSON 字符串；这样命令面不需要为每种
// 参数类型生成绑定，和鸿蒙那条完全同构。
/** 构建期为 GTK 平台时**再确认桥真的存在**。
 *
 * 只看构建期常量不够：同一份 dist 也可能被别的 webview 打开（比如用 CEF/Qt
 * 做视觉对比时），那时 `window.webkit` 根本不存在，界面却会一直报
 * 「GTK 宿主桥尚未就绪」而挡住整页。真正的判据是 WebKit 注入的
 * `messageHandlers.openless`。 */
const platformIsGtk = import.meta.env?.VITE_OPENLESS_PLATFORM === 'gtk';

export function gtkBridgeAvailable(): boolean {
  return (
    platformIsGtk &&
    typeof window !== 'undefined' &&
    !!window.webkit?.messageHandlers?.openless
  );
}

export const isGtkHost = gtkBridgeAvailable();

// 登记到事件总线（单向依赖：桥 → 总线，总线不反过来 import 桥）。
if (isGtkHost) {
  void import('./hostEventBus').then((module) => module.markNativeHost());
}

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

// 引擎标记：WebKitGTK 缺几样合成器能力（全页滤镜动画最明显），
// 前端与 CSS 据此降级。由构建期的 isGtkHost 决定，不依赖宿主注入脚本。
if (isGtkHost && typeof document !== 'undefined') {
  document.documentElement.dataset.olEngine = 'webkitgtk';
}

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
