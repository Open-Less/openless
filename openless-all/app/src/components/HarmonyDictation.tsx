import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { invokeHarmony } from '../lib/harmonyBridge';
import { copyTextToClipboard } from '../lib/ipc';
import { Btn, Card } from '../pages/_atoms';

interface VoiceState {
  phase: 'idle' | 'starting' | 'recording' | 'processing' | 'cancelling';
  level: number;
  error: string;
}
interface VoiceResult {
  rawText: string;
  polishedText: string;
}

export function HarmonyDictation({ onComplete }: { onComplete: () => void }) {
  const { t } = useTranslation();
  const [state, setState] = useState<VoiceState>({ phase: 'idle', level: 0, error: '' });
  const [result, setResult] = useState('');
  const [copied, setCopied] = useState(false);
  const generation = useRef(0);
  const active = useRef(false);
  const busy = useRef(false);
  useEffect(() => {
    const update = (event: Event) => {
      const next = (event as CustomEvent<VoiceState>).detail;
      if (
        next &&
        ['idle', 'starting', 'recording', 'processing', 'cancelling'].includes(next.phase)
      ) {
        active.current = next.phase !== 'idle';
        setState(next);
      }
    };
    window.addEventListener('openless:harmony-dictation', update);
    return () => {
      generation.current++;
      window.removeEventListener('openless:harmony-dictation', update);
      if (active.current) void invokeHarmony('cancel_dictation').catch(() => {});
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
        const reply = await invokeHarmony<VoiceResult>('stop_dictation');
        if (epoch !== generation.current) return;
        active.current = false;
        setResult(reply.polishedText || reply.rawText);
        setState({ phase: 'idle', level: 0, error: '' });
        onComplete();
      } else {
        await invokeHarmony('start_dictation');
        if (epoch === generation.current) setState({ phase: 'recording', level: 0, error: '' });
      }
    } catch (error) {
      if (epoch === generation.current) {
        active.current = false;
        setState({ phase: 'idle', level: 0, error: String(error) });
      }
    } finally {
      if (epoch === generation.current) busy.current = false;
    }
  };
  const cancel = async () => {
    const epoch = ++generation.current;
    busy.current = true;
    setState({ phase: 'cancelling', level: 0, error: '' });
    try {
      await invokeHarmony('cancel_dictation');
    } catch (error) {
      if (epoch === generation.current) setState({ phase: 'idle', level: 0, error: String(error) });
      return;
    } finally {
      if (epoch === generation.current) {
        busy.current = false;
        active.current = false;
      }
    }
    if (epoch === generation.current) setState({ phase: 'idle', level: 0, error: '' });
  };
  const copy = async () => {
    try {
      await copyTextToClipboard(result);
      setCopied(true);
    } catch (error) {
      setState((old) => ({ ...old, error: String(error) }));
    }
  };
  const waiting = ['starting', 'processing', 'cancelling'].includes(state.phase);
  return (
    <Card padding={16} style={{ flexShrink: 0 }}>
      <div style={{ display: 'flex', alignItems: 'center', flexWrap: 'wrap', gap: 12 }}>
        <strong style={{ flex: 1, fontSize: 14 }}>{t('overview.inAppDictation.title')}</strong>
        <span role="status" style={{ fontSize: 12, color: 'var(--ol-ink-3)' }}>
          {t(`overview.inAppDictation.${waiting ? 'processing' : state.phase}`)}
        </span>
        <Btn size="sm" onClick={() => void record()} disabled={waiting}>
          {t(`overview.inAppDictation.${state.phase === 'recording' ? 'stop' : 'start'}`)}
        </Btn>
        {state.phase !== 'idle' && (
          <Btn
            size="sm"
            variant="soft"
            disabled={state.phase === 'cancelling'}
            onClick={() => void cancel()}
          >
            {t('common.cancel')}
          </Btn>
        )}
      </div>
      {state.phase === 'recording' && (
        <progress
          aria-label={t('overview.inAppDictation.recording')}
          value={state.level}
          max={1}
          style={{ width: '100%', marginTop: 12 }}
        />
      )}
      {state.error && (
        <div role="alert" style={{ color: 'var(--ol-danger)', fontSize: 13, marginTop: 10 }}>
          {state.error}
        </div>
      )}
      {result && (
        <div style={{ marginTop: 12 }}>
          <div
            style={{
              whiteSpace: 'pre-wrap',
              overflowWrap: 'anywhere',
              maxHeight: 160,
              overflowY: 'auto',
              fontSize: 14,
            }}
          >
            {result}
          </div>
          <Btn size="sm" variant="soft" onClick={() => void copy()}>
            {t(copied ? 'common.copied' : 'common.copy')}
          </Btn>
        </div>
      )}
    </Card>
  );
}
