import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { invokeOrPreview, listenTauriEvent } from '../../runtime/transport';
import { copyTextToClipboard } from '../../api/files';
import { AssistantMarkdown } from './chat/markdown';
import { Button } from '../../ui/controls/Button';
import { useInteraction } from '../../ui/overlays/InteractionProvider';

interface Snapshot {
  phase: string;
  sessionId?: string;
  messages: { id: string; role: string; content: string }[];
  lastError?: string;
}

/** View only. Core's existing QA service owns recording, turns and cancellation.
 * No attachment/system-context promises: those require the separate Ask host. */
export function LinuxQaWindow() {
  const { i18n } = useTranslation();
  const zh = i18n.language.startsWith('zh');
  const { notify } = useInteraction('linux-qa');
  const [snapshot, setSnapshot] = useState<Snapshot>({ phase: 'idle', messages: [] });
  const [draft, setDraft] = useState('');
  const [pending, setPending] = useState(false);
  const refresh = useRef<() => Promise<void>>(async () => {});
  useEffect(() => {
    let active = true, requested = 0, unsubscribe = () => {};
    refresh.current = async () => {
      const request = ++requested;
      try {
        const next = await invokeOrPreview<Snapshot>('qa_get_snapshot', undefined, () => ({ phase: 'idle', messages: [] }));
        if (active && request === requested) setSnapshot(next);
      } catch (error) {
        if (active) notify({ key: 'linux-qa-load', state: 'failed', message: String(error) });
      }
    };
    void listenTauriEvent('qa:state', () => { void refresh.current(); }).then((off) => {
      if (!active) off(); else { unsubscribe = off; void refresh.current(); }
    }).catch((error) => { if (active) notify({ state: 'failed', message: String(error) }); });
    return () => { active = false; unsubscribe(); };
  }, [notify]);
  const run = async (command: string, args?: Record<string, unknown>) => {
    try { await invokeOrPreview(command, args, () => undefined); }
    catch (error) { notify({ state: 'failed', message: String(error) }); }
    finally { await refresh.current(); }
  };
  const working = snapshot.phase === 'thinking' || snapshot.phase === 'recording';
  return (
    <section style={{ background: 'var(--ol-surface)', color: 'var(--ol-ink)', borderRadius: 12, padding: 16, height: '100dvh', boxSizing: 'border-box', display: 'flex', flexDirection: 'column', gap: 10 }}>
      <header style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
        <strong>{zh ? '问答' : 'Questions'}</strong>
        <Button size="sm" onClick={() => void run('qa_window_dismiss')}>{zh ? '关闭' : 'Close'}</Button>
      </header>
      <div aria-live="polite" style={{ overflow: 'auto', flex: 1, minHeight: 0, overflowWrap: 'anywhere' }}>
        {!snapshot.messages.length && <p>{zh ? '输入问题或点击语音开始提问。' : 'Type a question or start a voice question.'}</p>}
        {snapshot.messages.map((message) => <article key={message.id} style={{ marginBottom: 12 }}>
          <small>{message.role === 'user' ? (zh ? '你' : 'You') : 'OpenLess'}</small>
          <AssistantMarkdown markdown={message.content} streaming={snapshot.phase === 'thinking'} />
          {message.content && <Button size="sm" onClick={() => { void copyTextToClipboard(message.content).catch((error) => notify({ state: 'failed', message: String(error) })); }}>{zh ? '复制' : 'Copy'}</Button>}
        </article>)}
        {snapshot.lastError && <p role="alert">{snapshot.lastError}</p>}
      </div>
      <textarea aria-label={zh ? '问题' : 'Question'} value={draft} onChange={(event) => setDraft(event.target.value)} rows={2} disabled={working || pending} style={{ resize: 'none', minWidth: 0, width: '100%', boxSizing: 'border-box' }} />
      <div style={{ display: 'flex', gap: 8 }}>
        <Button disabled={!draft.trim() || pending || working} onClick={() => {
          const text = draft.trim(); setDraft(''); setPending(true);
          void run('qa_submit_text', { text, expectedSessionId: snapshot.sessionId }).finally(() => setPending(false));
        }}>{zh ? '发送' : 'Send'}</Button>
        <Button disabled={snapshot.phase === 'thinking' || pending} onClick={() => void run(snapshot.phase === 'recording' ? 'qa_stop_recording' : 'qa_toggle_recording')}>
          {snapshot.phase === 'recording' ? (zh ? '结束录音' : 'Finish recording') : (zh ? '语音提问' : 'Voice question')}
        </Button>
        {(working || pending) && <Button onClick={() => void run('qa_cancel_turn')}>{zh ? '取消本轮' : 'Cancel turn'}</Button>}
      </div>
    </section>
  );
}
