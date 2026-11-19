import { useEffect, useRef } from 'react';
import { listenQaDictationDraft, setQaDraftFocus } from '../../api/qa';

/** Own the editor generation and insert text without invoking the form. */
export function useQaDraftDictation(enabled: boolean, onChange: (text: string) => void) {
  const input = useRef<HTMLTextAreaElement>(null);
  const token = useRef<string>();
  const change = useRef(onChange);
  change.current = onChange;

  const setFocused = (focused: boolean) => {
    if (enabled && token.current) {
      void setQaDraftFocus(token.current, focused).catch(() => undefined);
    }
  };

  useEffect(() => {
    if (!enabled) return;
    // getRandomValues is available to native webviews on custom app schemes.
    const id = Array.from(crypto.getRandomValues(new Uint8Array(16)), (byte) =>
      byte.toString(16).padStart(2, '0'),
    ).join('');
    let stopped = false;
    let unlisten: (() => void) | undefined;
    const focus = () => setFocused(document.activeElement === input.current);
    const blur = () => setFocused(false);
    window.addEventListener('focus', focus);
    window.addEventListener('blur', blur);
    void listenQaDictationDraft(({ token: sender, text }) => {
      const editor = input.current;
      if (stopped || sender !== id || !editor || !text) return;
      editor.setRangeText(text, editor.selectionStart, editor.selectionEnd, 'end');
      change.current(editor.value);
    })
      .then((remove) => {
        if (stopped) remove();
        else {
          unlisten = remove;
          token.current = id;
          focus();
        }
      })
      .catch(() => undefined);
    return () => {
      stopped = true;
      unlisten?.();
      blur();
      token.current = undefined;
      window.removeEventListener('focus', focus);
      window.removeEventListener('blur', blur);
    };
  }, [enabled]);

  return { input, setFocused };
}
