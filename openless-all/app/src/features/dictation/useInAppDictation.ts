import { useEffect, useRef, useState } from 'react';
import { cancelDictation, startDictation } from '../../api/dictation';
import { copyTextToClipboard } from '../../api/files';
import {
  finishInAppDictation,
  listenInAppDictation,
  type VoiceState,
} from '../../api/in-app-dictation';

let pendingRelease: Promise<void> | null = null;

function releaseCapture(): Promise<void> {
  pendingRelease ??= cancelDictation().finally(() => {
    pendingRelease = null;
  });
  return pendingRelease;
}

export function useInAppDictation(onComplete: () => void) {
  const [state, setState] = useState<VoiceState>({ phase: 'idle', level: 0, error: '' });
  const [result, setResult] = useState('');
  const [copied, setCopied] = useState(false);
  const generation = useRef(0);
  const active = useRef(false);
  const busy = useRef(false);
  const cancelling = useRef(false);
  const nativePhase = useRef<VoiceState['phase']>('idle');
  useEffect(() => {
    const unlisten = listenInAppDictation((next) => {
      nativePhase.current = next.phase;
      active.current = next.phase !== 'idle';
      if (!cancelling.current) setState(next);
    });
    return () => {
      generation.current++;
      unlisten();
      if (active.current) {
        void releaseCapture().catch((error) =>
          console.warn('[dictation] capture release failed', error),
        );
      }
    };
  }, []);

  const record = async () => {
    if (busy.current) return;
    busy.current = true;
    const epoch = ++generation.current;
    const stopping = state.phase === 'recording';
    active.current = true;
    setState({ phase: stopping ? 'processing' : 'starting', level: 0, error: '' });
    if (!stopping) {
      setResult('');
      setCopied(false);
    }
    try {
      if (stopping) {
        const reply = await finishInAppDictation();
        if (epoch !== generation.current) return;
        active.current = false;
        setResult(reply.polishedText || reply.rawText);
        setState({ phase: 'idle', level: 0, error: '' });
        onComplete();
      } else {
        if (pendingRelease) await pendingRelease;
        if (epoch !== generation.current) return;
        await startDictation();
        if (epoch === generation.current) setState({ phase: 'recording', level: 0, error: '' });
      }
    } catch (error) {
      if (epoch === generation.current) {
        const phase = nativePhase.current === 'recording' ? 'recording' : 'idle';
        active.current = phase !== 'idle';
        setState({ phase, level: 0, error: String(error) });
      }
    } finally {
      if (epoch === generation.current) busy.current = false;
    }
  };
  const cancel = async () => {
    if (cancelling.current) return;
    const epoch = ++generation.current;
    cancelling.current = true;
    busy.current = true;
    setState({ phase: 'cancelling', level: 0, error: '' });
    try {
      await releaseCapture();
      if (epoch !== generation.current) return;
      active.current = false;
      setState({ phase: 'idle', level: 0, error: '' });
    } catch (error) {
      if (epoch !== generation.current) return;
      // Keep cancel/retry reachable when the bridge did not confirm release.
      setState({ phase: active.current ? 'recording' : 'idle', level: 0, error: String(error) });
    } finally {
      if (epoch === generation.current) {
        busy.current = false;
        cancelling.current = false;
      }
    }
  };
  const copy = async () => {
    const epoch = generation.current;
    try {
      await copyTextToClipboard(result);
      if (epoch === generation.current) setCopied(true);
    } catch (error) {
      if (epoch === generation.current) setState((old) => ({ ...old, error: String(error) }));
    }
  };
  return { state, result, copied, record, cancel, copy };
}
