import { invokeOrMock } from './shared';

export interface SelectionVoicePreview {
  text: string;
  sourceText: string;
  summary?: string | null;
}

export function getSelectionVoicePreview(): Promise<SelectionVoicePreview | null> {
  return invokeOrMock('get_selection_voice_preview', undefined, () => ({
    text: '这里显示编辑后的文字。',
    sourceText: '这里显示原始选区。',
    summary: '批量替换邮箱域名',
  }));
}

export function confirmSelectionVoicePreview(text: string): Promise<void> {
  return invokeOrMock('confirm_selection_voice_preview', { text }, () => undefined);
}

export function cancelSelectionVoicePreview(): Promise<void> {
  return invokeOrMock('cancel_selection_voice_preview', undefined, () => undefined);
}
