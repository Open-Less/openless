import { useEffect, useState } from 'react';
import { invokeOrMock, isTauri } from '../lib/ipc';
import type { CapsulePayload, CapsuleState } from '../lib/types';

export interface CapsuleController {
  state: CapsuleState;
  level: number;
  insertedChars: number;
  message?: string;
  translation: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}

export function useCapsuleState(): CapsuleController {
  const [state, setState] = useState<CapsuleState>(isTauri ? 'idle' : 'recording');
  const [level, setLevel] = useState<number>(isTauri ? 0 : 0.6);
  const [insertedChars, setInsertedChars] = useState<number>(0);
  const [message, setMessage] = useState<string | undefined>();
  const [translation, setTranslation] = useState<boolean>(false);

  useEffect(() => {
    if (!isTauri) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    (async () => {
      const { listen } = await import('@tauri-apps/api/event');
      const handle = await listen<CapsulePayload>('capsule:state', event => {
        const payload = event.payload;
        setState(payload.state);
        setLevel(payload.level ?? 0);
        setMessage(payload.message ?? undefined);
        if (payload.insertedChars != null) setInsertedChars(payload.insertedChars);
        setTranslation(payload.translation === true);
      });
      if (cancelled) handle();
      else unlisten = handle;
    })();
    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, []);

  const onCancel = () => {
    void invokeOrMock<void>('cancel_dictation', undefined, () => undefined);
  };

  const onConfirm = () => {
    void invokeOrMock<void>('stop_dictation', undefined, () => undefined);
  };

  return {
    state,
    level,
    insertedChars,
    message,
    translation,
    onCancel,
    onConfirm,
  };
}
