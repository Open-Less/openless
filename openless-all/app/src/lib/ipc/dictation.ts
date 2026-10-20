import { isDesktop } from './platform-exports';
import { invokeOrMock, platformCapabilities } from './shared';
import type { CapsuleSnapshot } from '../types';

export function startDictation(): Promise<void> {
  return invokeOrMock('start_dictation', undefined, () => undefined);
}

export function stopDictation(): Promise<void> {
  return invokeOrMock('stop_dictation', undefined, () => undefined);
}

export function cancelDictation(): Promise<void> {
  return invokeOrMock('cancel_dictation', undefined, () => undefined);
}

export function setCapsuleTranscriptVisible(visible: boolean): Promise<void> {
  if (!isDesktop()) return Promise.resolve();
  return invokeOrMock('set_capsule_transcript_visible', { visible }, () => undefined);
}

/** Latest native capsule frame, used by the separate Windows rail after its webview is ready. */
export function getCapsuleSnapshot(): Promise<CapsuleSnapshot | null> {
  if (!isDesktop()) return Promise.resolve(null);
  return invokeOrMock('get_capsule_snapshot', undefined, () => null);
}

export function handleWindowHotkeyEvent(
  eventType: 'keydown' | 'keyup',
  key: string,
  code: string,
  repeat: boolean,
): Promise<void> {
  return platformCapabilities().then((caps) => {
    if (!caps.supportsDesktopHotkey) {
      return undefined;
    }
    return invokeOrMock(
      'handle_window_hotkey_event',
      { event_type: eventType, key, code, repeat },
      () => undefined,
    );
  });
}
