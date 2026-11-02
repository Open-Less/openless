import { isDesktop } from './platform-exports';
import { invokeOrMock, isTauriNow, platformCapabilities } from './shared';
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

export async function setCapsuleTranscriptVisible(visible: boolean): Promise<void> {
  if (!isDesktop()) return;
  if (isTauriNow()) {
    const { getCurrentWebviewWindow } = await import('@tauri-apps/api/webviewWindow');
    // Mirrors render the shared transcript but never own the primary rail's native layout.
    // This also prevents a retained/closing mirror's cleanup from hiding the active rail.
    if (getCurrentWebviewWindow().label !== 'capsule') return;
  }
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

export interface DictationTranslationTarget {
  sessionId: string;
  targetLanguage: string;
  workingLanguages: string[];
  mutable: boolean;
}

export function getDictationTranslationTarget(): Promise<DictationTranslationTarget | null> {
  return invokeOrMock('get_dictation_translation_target', undefined, () => null);
}

export function selectDictationTranslationTarget(
  sessionId: string,
  targetLanguage: string,
): Promise<DictationTranslationTarget> {
  return invokeOrMock('select_dictation_translation_target', { sessionId, targetLanguage }, () => {
    throw new Error('No active native recording');
  });
}

/** Reserve native headroom for the shared Base UI popup; close is session-bound. */
export function setCapsuleLanguageMenu(open: boolean, sessionId: string): Promise<void> {
  return invokeOrMock('set_capsule_language_menu', { open, sessionId }, () => undefined);
}
