import { invoke } from '@tauri-apps/api/core';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import type { AskClient, AskEvent, AskVoiceEvent, AskWindowEvent } from './client';

async function onWindowMessage<Payload>(event: string, handler: (payload: Payload) => void) {
  return getCurrentWebviewWindow().listen<Payload>(event, ({ payload }) => handler(payload));
}

export const tauriAskClient: AskClient = {
  current: () => invoke('ask_current'),
  newConversation: () => invoke('ask_new_conversation'),
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
  syncEditor: (editor) => invoke('ask_sync_editor', { editor }),
  acknowledgeVoice: (recordingId) => invoke('ask_ack_voice', { recordingId }),
  onEvent: (handler) => onWindowMessage<AskEvent>('ask:event', handler),
  onWindowEvent: (handler) => onWindowMessage<AskWindowEvent>('ask:window', handler),
  onVoiceEvent: (handler) => onWindowMessage<AskVoiceEvent>('ask:voice', handler),
};
