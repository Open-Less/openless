import { createRoot } from 'react-dom/client';
import '../../src/styles/tokens.css';
import '../../src/styles/global.css';
import '../../src/styles/overlays.css';
const params = new URLSearchParams(location.search);
const callbacks = new Map<number, (value: unknown) => void>();
const listeners = new Map<number, { event: string; handler: number }>();
const calls: { command: string; args: any }[] = [];
let next = 0;
let snapshot = { phase: 'idle', messages: [] as { id: string; role: string; content: string }[] };
Object.assign(window, { fixture: { calls, emit() {
  for (const [id, listener] of listeners) if (listener.event === 'qa:state') callbacks.get(listener.handler)?.({ event: 'qa:state', id, payload: { kind: snapshot.phase } });
} } });
window.__TAURI_INTERNALS__ = {
  metadata: { currentWindow: { label: 'qa' }, currentWebview: { label: 'qa' } },
  transformCallback(callback: (value: unknown) => void) { const id = ++next; callbacks.set(id, callback); return id; },
  unregisterCallback(id: number) { callbacks.delete(id); },
  async invoke(command: string, args: any) {
    calls.push({ command, args });
    if (command === 'get_startup_snapshot') return { contractVersion: '2.0.0', backend: { running: true } };
    if (command === 'plugin:event|listen') { const id = ++next; listeners.set(id, { event: args.event, handler: args.handler }); return id; }
    if (command === 'plugin:event|unlisten') { listeners.delete(args.eventId); return; }
    if (command === 'qa_get_snapshot') return structuredClone(snapshot);
    if (command === 'list_correction_rules') return [];
    if (command === 'qa_submit_text') {
      snapshot = { phase: 'completed', messages: [{ id: 'q', role: 'user', content: args.text }, { id: 'a', role: 'assistant', content: '测试回答 / test answer' }] };
      (window as any).fixture.emit();
    }
  },
} as any;
const { default: i18n, i18nReady } = await import('../../src/i18n');
await i18nReady;
await i18n.changeLanguage('zh-CN');
const { InteractionProvider, useInteraction } = await import('../../src/ui/overlays/InteractionProvider');
const { LinuxQaWindow } = await import('../../src/features/qa/LinuxQaWindow');
const { RestoreReview } = await import('../../src/features/cloud-sync/RestoreReview');
const { Corrections } = await import('../../src/features/corrections/Corrections');
function Notices() {
  const { notify } = useInteraction();
  return <button onClick={() => notify({ severity: 'warning', message: '会自动关闭的警告' })}>Warn</button>;
}
const preview = {
  previewId: 'p', observedRevision: '1', localGeneration: '1', counts: {}, deviceSettingsToReview: [], unconfirmedOperationId: null,
  conflicts: [
    { id: 'one', kind: 'style_packs', reason: 'both_modified', localLabel: '本地写作风格', cloudLabel: '云端写作风格' },
    { id: 'two', kind: 'style_packs', reason: 'both_modified', localLabel: '翻译风格', cloudLabel: '翻译风格' },
  ],
};
createRoot(document.getElementById('root')!).render(<InteractionProvider>
  {params.get('mode') === 'review' ? <RestoreReview preview={preview} busy={false} stale={false} onApply={() => {}} /> :
   params.get('mode') === 'notices' ? <Notices /> : params.get('mode') === 'corrections' ? <Corrections /> : <LinuxQaWindow />}
</InteractionProvider>);
