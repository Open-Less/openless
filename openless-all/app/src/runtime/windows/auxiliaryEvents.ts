import { isTauriHost } from '../environment';
import { requireBackendReady } from '../transport';

type EventScope = 'window' | 'app';

async function connect() {
  if (!isTauriHost()) return null;
  await requireBackendReady();
  const [{ listen }, { getCurrentWebviewWindow }] = await Promise.all([
    import('@tauri-apps/api/event'),
    import('@tauri-apps/api/webviewWindow'),
  ]);
  return { listen, label: getCurrentWebviewWindow().label };
}

export function createAuxiliaryWindowSubscriptions() {
  let source: ReturnType<typeof connect> | undefined;
  let closed = false;
  const listeners: (() => void)[] = [];
  const getSource = () => (source ??= connect());

  return {
    async label() {
      if (closed) return;
      const connection = await getSource();
      return closed ? undefined : connection?.label;
    },
    async listen<T>(name: string, onPayload: (payload: T) => void, scope: EventScope) {
      if (closed) return false;
      const connection = await getSource();
      if (closed || !connection) return false;
      // A mirror's targeted lifecycle events never change the primary window.
      const target =
        scope === 'window'
          ? { kind: 'WebviewWindow' as const, label: connection.label }
          : undefined;
      const unsubscribe = await connection.listen<T>(
        name,
        (event) => {
          if (!closed) onPayload(event.payload);
        },
        target ? { target } : undefined,
      );
      if (closed) {
        unsubscribe();
        return false;
      }
      listeners.push(unsubscribe);
      return true;
    },
    dispose() {
      closed = true;
      for (const unsubscribe of listeners.splice(0)) unsubscribe();
    },
  };
}
