import { getRuntimeEnvironment } from '../environment';
import { requireBackendReady } from '../transport';

export async function listenMainWindowFocus(onFocus: () => void): Promise<() => void> {
  if (getRuntimeEnvironment().host !== 'tauri') return () => {};
  await requireBackendReady();
  const { getCurrentWindow } = await import('@tauri-apps/api/window');
  return getCurrentWindow().onFocusChanged(({ payload }) => {
    if (payload) onFocus();
  });
}
