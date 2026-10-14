import { zhCN } from '../../i18n/zh-CN';
import { zhTW } from '../../i18n/zh-TW';
import { en } from '../../i18n/en';
import { ja } from '../../i18n/ja';
import { ko } from '../../i18n/ko';
import { de } from '../../i18n/de';
import { fr } from '../../i18n/fr';
import { es } from '../../i18n/es';

let calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
let cancelFails = false;
let currentSessionId: string | null = null;

export {};

Object.defineProperty(globalThis, 'window', {
  configurable: true,
  value: {
    __TAURI_INTERNALS__: {
      invoke: async (command: string, args?: Record<string, unknown>) => {
        calls.push({ command, args });
        if (command === 'get_startup_snapshot') {
          return { contractVersion: '2.0.0', backend: { running: true } };
        }
        if (command === 'get_voice_edit_state' && !currentSessionId) return null;
        if (command === 'start_voice_edit_session') currentSessionId = 'session-1';
        if (command === 'cancel_voice_edit_session') {
          if (cancelFails) throw new Error('cancel failed');
          currentSessionId = null;
          return null;
        }
        return {
          sessionId: currentSessionId,
          phase: command === 'start_voice_edit_session' ? 'dictating' : 'preview',
          context: null,
          target: { kind: 'full_field' },
        };
      },
    },
  },
});

try {
  const {
    startVoiceEditSession,
    openVoiceEditWindow,
    closeVoiceEditWindow,
    finalizeVoiceEditDictation,
    startVoiceEditInstruction,
    finalizeVoiceEditInstruction,
    commitVoiceEditSession,
    cancelVoiceEditSession,
    cancelAndCloseVoiceEditSession,
    getVoiceEditState,
  } = await import('./voice-edit-session');

  await openVoiceEditWindow();
  await startVoiceEditSession();
  await finalizeVoiceEditDictation('session-1');
  await startVoiceEditInstruction('session-1');
  await finalizeVoiceEditInstruction('session-1');
  await commitVoiceEditSession('session-1');
  await cancelVoiceEditSession('session-1');
  if ((await getVoiceEditState()) !== null) throw new Error('state query did not return null');
  await closeVoiceEditWindow();

  const expected = [
    'get_startup_snapshot',
    'voice_edit_window_open',
    'start_voice_edit_session',
    'finalize_voice_edit_dictation',
    'start_voice_edit_instruction',
    'stop_voice_edit_instruction',
    'commit_voice_edit',
    'cancel_voice_edit_session',
    'get_voice_edit_state',
    'voice_edit_window_close',
  ];
  if (JSON.stringify(calls.map(({ command }) => command)) !== JSON.stringify(expected)) {
    throw new Error(`unexpected voice edit command sequence: ${JSON.stringify(calls)}`);
  }
  for (const call of calls.slice(3, 8)) {
    if (call.args?.sessionId !== 'session-1') {
      throw new Error(`${call.command} must bind to the current session`);
    }
  }
  if (Object.keys(calls[2].args ?? {}).length)
    throw new Error('start must use native field capture');

  calls = [];
  cancelFails = true;
  try {
    await cancelAndCloseVoiceEditSession('session-1');
    throw new Error('cancellation failure must reject close');
  } catch (error) {
    if (!(error instanceof Error) || error.message !== 'cancel failed') throw error;
  }
  if (calls.length !== 1 || calls[0].command !== 'cancel_voice_edit_session') {
    throw new Error('failed cancellation must preserve the visible panel');
  }

  calls = [];
  cancelFails = false;
  currentSessionId = 'starting-session';
  await cancelAndCloseVoiceEditSession();
  if (
    calls.map(({ command }) => command).join(',') !==
    'get_voice_edit_state,cancel_voice_edit_session,voice_edit_window_close'
  ) {
    throw new Error('close must query and cancel a pending startup before hiding');
  }
  if (
    calls[1].args?.sessionId !== 'starting-session' ||
    calls[2].args?.sessionId !== 'starting-session'
  ) {
    throw new Error('close must bind cancellation to the captured session');
  }
} finally {
  Reflect.deleteProperty(globalThis, 'window');
}

for (const [locale, messages] of Object.entries({ zhCN, zhTW, en, ja, ko, de, fr, es })) {
  if (locale !== 'en' && messages.voiceEdit.title === en.voiceEdit.title) {
    throw new Error(`${locale} voice edit copy must not be overwritten by the English fallback`);
  }
  if (Object.keys(messages.voiceEdit).join(',') !== Object.keys(zhCN.voiceEdit).join(',')) {
    throw new Error(`${locale} must define every voice edit message`);
  }
  if (
    Object.keys(messages.voiceEdit.phase).join(',') !== Object.keys(zhCN.voiceEdit.phase).join(',')
  ) {
    throw new Error(`${locale} must translate every voice edit phase`);
  }
  if (
    Object.keys(messages.voiceEdit.errors).join(',') !==
    Object.keys(zhCN.voiceEdit.errors).join(',')
  ) {
    throw new Error(`${locale} must translate every voice edit error`);
  }
  if (Object.values(messages.voiceEdit.errors).some((message) => !message)) {
    throw new Error(`${locale} voice edit errors must not be empty`);
  }
  for (const [key, message] of Object.entries(messages.voiceEdit)) {
    if (typeof message !== 'string') continue;
    const reference = zhCN.voiceEdit[key as keyof typeof zhCN.voiceEdit];
    if (typeof reference !== 'string' || !message) throw new Error(`${locale}.${key} is empty`);
    if (
      JSON.stringify(message.match(/\{\{\w+\}\}/g)) !==
      JSON.stringify(reference.match(/\{\{\w+\}\}/g))
    ) {
      throw new Error(`${locale}.${key} must preserve interpolation parameters`);
    }
  }
}

console.log('voice-edit-session.test.ts passed');
