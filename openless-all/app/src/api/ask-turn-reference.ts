import { invokeOrPreview } from '../runtime/transport';

export type AskReadPermissions = {
  browserTab: boolean;
  browserSelection: boolean;
  voiceHistory: boolean;
};

export type AskTurnReference = {
  source: 'userProvided' | 'originalInputAccessibility';
  temporary: true;
  /** Omitted after manual material has been consumed; retain only in this conversation's memory. */
  text?: string;
  characters?: number;
  active?: true;
};

export type AskTurnReferenceReply = AskTurnReference & { conversationId: string };
export type AskReferenceInvalidated = { conversationId: string; reason: 'askReferenceUnavailable' };

export function setAskTurnReference(conversationId: string, text: string): Promise<AskTurnReferenceReply> {
  return invokeOrPreview<AskTurnReferenceReply>('ask_set_turn_reference', { conversationId, text }, () => ({
    conversationId: crypto.randomUUID(), source: 'userProvided', characters: [...text].length, temporary: true,
  }));
}

export function revokeAskTurnReference(conversationId: string): Promise<void> {
  return invokeOrPreview('ask_revoke_turn_reference', { conversationId }, () => undefined);
}
