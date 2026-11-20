import { useEffect } from 'react';
import { handleWindowHotkeyEvent } from '../api/dictation';
import {
  isWindowHotkeyKeyboardCandidate,
  windowMouseHotkeyCode,
} from '../lib/windowHotkeyFallback';
import { isTauriHost, type OS } from '../runtime/environment';

export function useMainWindowHotkeys(os: OS): void {
  useEffect(() => {
    if (!isTauriHost() || os !== 'win') return;
    const forwardKey = (event: KeyboardEvent) => {
      if (!isWindowHotkeyKeyboardCandidate(event)) return;
      void handleWindowHotkeyEvent(
        event.type as 'keydown' | 'keyup',
        event.key,
        event.code,
        event.repeat,
      ).catch((error) => console.warn('[window-hotkey] forward failed', error));
    };
    const forwardMouse = (event: MouseEvent) => {
      const code = windowMouseHotkeyCode(event.button);
      if (!code) return;
      void handleWindowHotkeyEvent(
        event.type === 'mousedown' ? 'keydown' : 'keyup',
        code,
        code,
        false,
      ).catch((error) => console.warn('[window-hotkey] mouse forward failed', error));
    };
    window.addEventListener('keydown', forwardKey, true);
    window.addEventListener('keyup', forwardKey, true);
    window.addEventListener('mousedown', forwardMouse, true);
    window.addEventListener('mouseup', forwardMouse, true);
    return () => {
      window.removeEventListener('keydown', forwardKey, true);
      window.removeEventListener('keyup', forwardKey, true);
      window.removeEventListener('mousedown', forwardMouse, true);
      window.removeEventListener('mouseup', forwardMouse, true);
    };
  }, [os]);
}
