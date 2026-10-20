export const isHarmonyHost = import.meta.env?.VITE_OPENLESS_PLATFORM === 'harmony';

declare global {
  interface Window {
    OpenLessHarmonyApi?: {
      invoke(command: string, args: string): string;
      request?(command: string, args: string): string;
      takeResult?(id: string): string;
    };
  }
}

export async function invokeHarmony<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  const bridge = window.OpenLessHarmonyApi;
  if (!bridge) throw new Error('鸿蒙原生后端桥尚未就绪');
  const wire =
    bridge.request && bridge.takeResult
      ? await new Promise<string>((resolve, reject) => {
          const id = bridge.request!(command, JSON.stringify(args ?? {}));
          const deadline = Date.now() + 120_000;
          const poll = () => {
            try {
              const result = bridge.takeResult!(id);
              if (result) resolve(result);
              else if (Date.now() >= deadline) reject(new Error(`鸿蒙命令超时：${command}`));
              else window.setTimeout(poll, 50);
            } catch (error) {
              reject(error);
            }
          };
          poll();
        })
      : bridge.invoke(command, JSON.stringify(args ?? {}));
  const result = JSON.parse(wire) as {
    ok: boolean;
    value?: T;
    error?: string;
  };
  if (!result.ok) throw new Error(result.error ?? '鸿蒙后端调用失败');
  return result.value as T;
}
