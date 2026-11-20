import { useEffect, useRef, useState } from 'react';
import {
  listenQaDictationDraft,
  listenQaDraftRecording,
  setQaDraftFocus,
  type QaDraftRecording,
} from '../../api/qa';

/** Own the editor generation and insert text without invoking the form. */
export function useQaDraftDictation(enabled: boolean, onChange: (text: string) => void) {
  const input = useRef<HTMLTextAreaElement>(null);
  const token = useRef<string>();
  const [recording, setRecording] = useState<QaDraftRecording | null>(null);
  const [error, setError] = useState<string | null>(null);
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
    const handles: (() => void)[] = [];
    const attach = async (listener: Promise<() => void>) => {
      const remove = await listener;
      if (stopped) remove();
      else handles.push(remove);
    };
    const focus = () => setFocused(document.activeElement === input.current);
    const blur = () => setFocused(false);
    window.addEventListener('focus', focus);
    window.addEventListener('blur', blur);
    void Promise.all([
      attach(
        listenQaDictationDraft(({ token: sender, text }) => {
          const editor = input.current;
          if (stopped || sender !== id || !editor || !text) return;
          editor.setRangeText(text, editor.selectionStart, editor.selectionEnd, 'end');
          change.current(editor.value);
        }),
      ),
      attach(
        listenQaDraftRecording((state) => {
          if (stopped || state.token !== id) return;
          const active = state.phase === 'recording' || state.phase === 'processing';
          if (active) setError(null);
          else if (state.phase === 'failed') setError(state.error ?? 'Internal');
          setRecording((current) =>
            active ? state : current?.sessionId === state.sessionId ? null : current,
          );
        }),
      ),
    ])
      .then(() => {
        if (stopped) return;
        token.current = id;
        focus();
      })
      .catch(() => undefined);
    return () => {
      stopped = true;
      handles.forEach((remove) => remove());
      blur();
      token.current = undefined;
      window.removeEventListener('focus', focus);
      window.removeEventListener('blur', blur);
    };
  }, [enabled]);

  return {
    input,
    setFocused,
    recording: enabled ? recording : null,
    error,
    clearError: () => setError(null),
  };
}
