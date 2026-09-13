import type { SelectionVoiceApplyOutcome } from './selection-voice-preview';

let fail = false;
const failure = new Error('selectionVoiceTargetUnavailable');
Object.defineProperty(globalThis, 'window', {
  configurable: true,
  value: {
    __TAURI_INTERNALS__: {
      invoke: async (command: string, args?: { text?: string; qaSessionId?: string }) => {
        if (command === 'get_startup_snapshot') {
          return { contractVersion: '2.0.0', backend: { running: true } };
        }
        if (
          command !== 'confirm_selection_voice_preview' ||
          args?.text !== 'replacement' ||
          args.qaSessionId !== 'qa-owner'
        ) {
          throw new Error('unexpected Selection Voice command or owner');
        }
        if (fail) throw failure;
        return 'paste_sent';
      },
    },
  },
});
try {
  const { confirmSelectionVoicePreview } = await import('./selection-voice-preview');
  const outcome: SelectionVoiceApplyOutcome = await confirmSelectionVoicePreview(
    'replacement',
    'qa-owner',
  );
  if (outcome !== 'paste_sent') throw new Error('paste dispatch was not preserved');
  fail = true;
  const error = await confirmSelectionVoicePreview('replacement', 'qa-owner').catch(
    (error) => error,
  );
  if (error !== failure) throw new Error('failed insertion was reported as success');
} finally {
  Reflect.deleteProperty(globalThis, 'window');
}
console.log('selection-voice-preview.test.ts passed');
