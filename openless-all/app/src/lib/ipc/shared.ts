import type {
  HotkeyCapability,
  HotkeyStatus,
  PlatformCapabilities,
  WindowsImeStatus,
} from '../types';
import { getPlatformCapabilities as loadPlatformCapabilities } from '../platform';
import { invokeHarmony, isHarmonyHost } from '../harmonyBridge';
import { invokeGtk, isGtkHost } from '../gtkBridge';
import { invokeHttp, isHttpBridge } from '../httpBridge';

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
  }
}

// A page/webview reload (observed on Android after a Wry window rebuild)
// re-evaluates this module. If that happens to run before Tauri's own
// injection of `__TAURI_INTERNALS__` completes, a plain `const` computed once
// here would stay permanently `false` for the rest of that page's lifetime,
// even once the bridge becomes available moments later — silently routing
// every subsequent backend call through the browser-preview mock forever.
// `isTauriNow()` re-checks live; `isTauri` is kept as a snapshot for the many
// call sites that only use it for one-time UI/render decisions.
export function isTauriNow(): boolean {
  return globalThis.window !== undefined && '__TAURI_INTERNALS__' in globalThis.window;
}
export const isTauri = isTauriNow();

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

let backendReadyPromise: Promise<StartupSnapshot> | null = null;

export function requireBackendReady(): Promise<StartupSnapshot> {
  if (previewMockMode()) {
    return Promise.resolve({
      contractVersion: BACKEND_CONTRACT_VERSION,
      backend: { running: true },
    });
  }
  if (isHarmonyHost) {
    backendReadyPromise ??=
      invokeHarmony<StartupSnapshot>('get_startup_snapshot').then(validateStartupSnapshot);
    return backendReadyPromise;
  }
  if (isHttpBridge) {
    backendReadyPromise ??=
      invokeHttp<StartupSnapshot>('get_startup_snapshot').then(validateStartupSnapshot);
    return backendReadyPromise;
  }
  if (isGtkHost) {
    backendReadyPromise ??=
      invokeGtk<StartupSnapshot>('get_startup_snapshot').then(validateStartupSnapshot);
    return backendReadyPromise;
  }
  if (!isTauriNow()) {
    return Promise.resolve({
      contractVersion: BACKEND_CONTRACT_VERSION,
      backend: { running: true },
    });
  }
  backendReadyPromise ??= import('@tauri-apps/api/core')
    .then(({ invoke }) => invoke<StartupSnapshot>('get_startup_snapshot'))
    .then(validateStartupSnapshot);
  return backendReadyPromise;
}

let platformCapsPromise: Promise<PlatformCapabilities> | null = null;

export async function platformCapabilities(): Promise<PlatformCapabilities> {
  platformCapsPromise ??= loadPlatformCapabilities();
  return platformCapsPromise;
}

export async function getPlatformCapabilities(): Promise<PlatformCapabilities> {
  return platformCapabilities();
}

/** 视觉预览模式：`?mock=1` 时强制走前端 mock 数据，不碰任何原生桥。
 *  用途：在任意 webview（GTK/CEF/Qt）里都能看到完整界面，方便对比渲染差异，
 *  而不需要把后端也接上。**只影响查询路径，不改变正式运行时的行为。** */
function previewMockMode(): boolean {
  if (typeof window === 'undefined') return false;
  return new URLSearchParams(window.location.search).get('mock') === '1';
}

export async function invokeOrMock<T>(
  cmd: string,
  args: Record<string, unknown> | undefined,
  mock: () => T,
): Promise<T> {
  if (previewMockMode()) {
    return mock();
  }
  if (isHarmonyHost) {
    await requireBackendReady();
    return invokeHarmony<T>(cmd, args);
  }
  // 通用 HTTP 桥（Qt / CEF / 浏览器直连本地后端）。
  if (isHttpBridge) {
    await requireBackendReady();
    return invokeHttp<T>(cmd, args);
  }
  // 本机 GTK4 + WebKitGTK 宿主：同样走"原生宿主"路径，不走浏览器 mock。
  if (isGtkHost) {
    await requireBackendReady();
    return invokeGtk<T>(cmd, args);
  }
  if (!isTauriNow()) {
    return mock();
  }
  if (cmd === 'get_startup_snapshot') {
    return requireBackendReady() as Promise<T>;
  }
  await requireBackendReady();
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(cmd, args);
}

export const androidHotkeyCapability: HotkeyCapability = {
  adapter: 'unavailable',
  availableTriggers: [],
  requiresAccessibilityPermission: false,
  supportsModifierOnlyTrigger: false,
  supportsSideSpecificModifiers: false,
  explicitFallbackAvailable: false,
  statusHint: '移动端不支持全局热键；请使用应用内录音按钮或悬浮窗（需授权）。',
};

export const androidHotkeyStatus: HotkeyStatus = {
  adapter: 'unavailable',
  state: 'failed',
  message: '移动端不支持全局热键',
  lastError: {
    code: 'unavailable',
    message: 'Global hotkeys are not available on mobile',
  },
};

export const androidWindowsImeStatus: WindowsImeStatus = {
  state: 'notWindows',
  usingTsfBackend: false,
  message: 'Not available on Android',
  dllPath: null,
};
