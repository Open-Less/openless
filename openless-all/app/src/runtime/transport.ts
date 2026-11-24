import { getRuntimeEnvironment } from './environment';
import { invokeHarmony } from '../lib/harmonyBridge';

export const BACKEND_CONTRACT_VERSION = '2.0.0';

export interface StartupSnapshot {
  contractVersion: string;
  backend: { running: boolean };
}

export function validateStartupSnapshot(snapshot: StartupSnapshot): StartupSnapshot {
  if (snapshot.contractVersion !== BACKEND_CONTRACT_VERSION) {
    throw new Error(`unsupported backend contract version: ${snapshot.contractVersion}`);
  }
  if (!snapshot.backend.running) {
    throw new Error('backend failed to start');
  }
  return snapshot;
}

export async function waitForNativeBridge(host: 'tauri' | 'harmony'): Promise<void> {
  const isReady = () =>
    host === 'harmony'
      ? typeof globalThis.window?.OpenLessHarmonyApi?.invoke === 'function'
      : typeof (globalThis.window?.__TAURI_INTERNALS__ as { invoke?: unknown } | undefined)
          ?.invoke === 'function';
  const deadline = Date.now() + 2_000;
  while (!isReady()) {
    if (Date.now() >= deadline) throw new Error(`${host} backend bridge is not ready`);
    await new Promise<void>((resolve) => setTimeout(resolve, 20));
  }
}

async function invokeNative<T>(
  host: 'tauri' | 'harmony',
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (host === 'harmony') return invokeHarmony<T>(command, args);
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(command, args);
}

let backendReady: Promise<StartupSnapshot> | undefined;

export function requireBackendReady(): Promise<StartupSnapshot> {
  const { host } = getRuntimeEnvironment();
  if (host === 'preview') {
    return Promise.resolve({
      contractVersion: BACKEND_CONTRACT_VERSION,
      backend: { running: true },
    });
  }
  backendReady ??= waitForNativeBridge(host)
    .then(() => invokeNative<StartupSnapshot>(host, 'get_startup_snapshot'))
    .then(validateStartupSnapshot)
    .catch((error: unknown) => {
      // A later retry can wait for a bridge that was missing during the first boot.
      backendReady = undefined;
      throw error;
    });
  return backendReady;
}

export async function invokeOrPreview<T>(
  command: string,
  args: Record<string, unknown> | undefined,
  preview: () => T | Promise<T>,
): Promise<T> {
  const { host } = getRuntimeEnvironment();
  if (host === 'preview') return preview();
  if (command === 'get_startup_snapshot') return requireBackendReady() as Promise<T>;
  await requireBackendReady();
  return invokeNative<T>(host, command, args);
}

export async function listenTauriEvent<T>(
  name: string,
  onEvent: (payload: T) => void,
): Promise<() => void> {
  if (getRuntimeEnvironment().host !== 'tauri') return () => {};
  await requireBackendReady();
  const { listen } = await import('@tauri-apps/api/event');
  return listen<T>(name, (event) => onEvent(event.payload));
}

export const HARMONY_CHANGE_EVENT = 'openless:harmony-backend-change';
