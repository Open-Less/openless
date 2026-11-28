import { getPlatformCapabilities } from '../runtime/platform';
import { invokeOrMock } from '../lib/ipc/shared';

interface DictationReferencesStatus {
  supported: boolean;
}

const unavailable: DictationReferencesStatus = { supported: false };

export async function getDictationReferencesStatus(): Promise<DictationReferencesStatus> {
  try {
    if ((await getPlatformCapabilities()).platform !== 'desktop') return unavailable;
    const { supported } = await invokeOrMock<DictationReferencesStatus>(
      'get_dictation_references_status',
      undefined,
      () => ({ supported: true }),
    );
    return { supported: supported === true };
  } catch {
    return unavailable;
  }
}
