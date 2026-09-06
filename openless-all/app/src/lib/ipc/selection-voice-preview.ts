import { invokeOrMock } from './shared';

export interface SelectionVoicePreview {
  text: string;
  sourceText: string;
  summary?: string | null;
}

export interface SelectionVoiceIntentPrompt {
  instruction: string;
  sourceText: string;
}

export function getSelectionVoiceIntentPrompt(): Promise<SelectionVoiceIntentPrompt | null> {
  return invokeOrMock('get_selection_voice_intent_prompt', undefined, () => ({
    instruction: '把邮箱批量替换成公司域名',
    sourceText: 'alice@old.com, bob@old.com',
  }));
}

export function confirmSelectionVoiceIntentPrompt(intent: 'question' | 'edit'): Promise<void> {
  return invokeOrMock('confirm_selection_voice_intent_prompt', { intent }, () => undefined);
}

export function cancelSelectionVoiceIntentPrompt(): Promise<void> {
  return invokeOrMock('cancel_selection_voice_intent_prompt', undefined, () => undefined);
}

export function getSelectionVoicePreview(qaSessionId: string): Promise<SelectionVoicePreview | null> {
  return invokeOrMock('get_selection_voice_preview', { qaSessionId }, () => ({
    text: '这里显示编辑后的文字。',
    sourceText: '这里显示原始选区。',
    summary: '批量替换邮箱域名',
  }));
}

export type SelectionVoiceApplyOutcome = 'inserted' | 'paste_sent' | 'copied_fallback';

export function confirmSelectionVoicePreview(text: string, qaSessionId: string): Promise<SelectionVoiceApplyOutcome> {
  return invokeOrMock('confirm_selection_voice_preview', { text, qaSessionId }, () => 'paste_sent');
}

export function revertSelectionVoicePreview(qaSessionId: string): Promise<void> {
  return invokeOrMock('revert_selection_voice_preview', { qaSessionId }, () => undefined);
}
