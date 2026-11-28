import { getPlatformCapabilities } from '../runtime/platform';
import { invokeOrMock } from '../lib/ipc/shared';

export interface DictationReferencesStatus {
  supported: boolean;
  screenCapture: 'granted' | 'denied' | 'unknown';
}

const unavailable: DictationReferencesStatus = { supported: false, screenCapture: 'unknown' };

export async function getDictationReferencesStatus(): Promise<DictationReferencesStatus> {
  try {
    if ((await getPlatformCapabilities()).platform !== 'desktop') return unavailable;
    return await invokeOrMock<DictationReferencesStatus>(
      'get_dictation_references_status',
      undefined,
      () => ({ supported: true, screenCapture: 'granted' }),
    );
  } catch {
    return unavailable;
  }
}

/** Returns false if the host cannot open the permission pane. */
export async function openDictationReferencesPermissionSettings(): Promise<boolean> {
  try {
    if ((await getPlatformCapabilities()).platform !== 'desktop') return false;
    await invokeOrMock<void>('open_dictation_references_permission_settings', undefined, () => {});
    return true;
  } catch {
    return false;
  }
}
