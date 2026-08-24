import { invokeOrMock } from './shared';

export interface SelectionPolishPreview {
  text: string;
  sourceText: string;
  reviewAnnotation: string | null;
}

export function getSelectionPolishPreview(): Promise<SelectionPolishPreview | null> {
  return invokeOrMock('get_selection_polish_preview', undefined, () => ({
    text: '这里显示润色后的文字。', sourceText: '这里显示原始选区。', reviewAnnotation: null,
  }));
}

export function confirmSelectionPolishPreview(text: string): Promise<void> {
  return invokeOrMock('confirm_selection_polish_preview', { text }, () => undefined);
}

export function cancelSelectionPolishPreview(): Promise<void> {
  return invokeOrMock('cancel_selection_polish_preview', undefined, () => undefined);
}
