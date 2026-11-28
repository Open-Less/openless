import { invoke } from '@tauri-apps/api/core';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import type { AskReferenceInvalidated } from '../src/api/ask-turn-reference';
import type { AskClient, AskEvent, AskVoiceEvent, AskWindowEvent } from './client';

async function onWindowMessage<Payload>(event: string, handler: (payload: Payload) => void) {
  return getCurrentWebviewWindow().listen<Payload>(event, ({ payload }) => handler(payload));
}

export const tauriAskClient: AskClient = {
  current: () => invoke('ask_current'),
  newConversation: () => invoke('ask_new_conversation'),
  // The isolated Ask window cannot call the main window's startup handshake.
  setTurnReference: (conversationId, text) => invoke('ask_set_turn_reference', { conversationId, text }),
  revokeTurnReference: (conversationId) => invoke('ask_revoke_turn_reference', { conversationId }),
  endConversation: (conversationId) => invoke('ask_end_conversation', { conversationId }),
  snapshot: (conversationId) => invoke('ask_snapshot', { conversationId }),
  events: (conversationId, afterSequence) =>
    invoke('ask_events', { conversationId, afterSequence }),
  submit: (request) => invoke('ask_submit', { request }),
  cancel: (conversationId, turnId) => invoke('ask_cancel', { conversationId, turnId }),
  addAttachment: (conversationId) => invoke('ask_add_attachment', { conversationId }),
  addDroppedImage: (conversationId, displayName, dataBase64) =>
    invoke('ask_add_dropped_image', { conversationId, displayName, dataBase64 }),
  removeAttachment: (conversationId, attachmentId) =>
    invoke('ask_remove_attachment', { conversationId, attachmentId }),
  getAttachmentPreview: (conversationId, attachmentId) =>
    invoke('ask_get_attachment_preview', { conversationId, attachmentId }),
  openSource: (conversationId, turnId, sourceId) =>
    invoke('ask_open_source', { conversationId, turnId, sourceId }),
  hide: () => invoke('ask_window_hide'),
  expandChat: (conversationId, turnId) => invoke('ask_expand_chat', { conversationId, turnId }),
  syncEditor: (focus, editor) => invoke('ask_sync_editor', { ...focus, editor }),
  acknowledgeVoice: (recordingId) => invoke('ask_ack_voice', { recordingId }),
  onEvent: (handler) => onWindowMessage<AskEvent>('ask:event', handler),
  async onWindowEvent(handler) {
    const stopWindow = await onWindowMessage<AskWindowEvent>('ask:window', handler);
    try {
      const stopReference = await onWindowMessage<AskReferenceInvalidated>(
        'ask-reference-invalidated',
        (event) => handler({ ...event, type: 'referenceInvalidated' }),
      );
      return () => {
        stopWindow();
        stopReference();
      };
    } catch (error) {
      stopWindow();
      throw error;
    }
  },
  onVoiceEvent: (handler) => onWindowMessage<AskVoiceEvent>('ask:voice', handler),
};
