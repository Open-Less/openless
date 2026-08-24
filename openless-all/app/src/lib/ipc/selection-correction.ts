import { invokeOrMock } from './shared';

export type SelectionCorrectionAction = 'literalReplace' | 'review';
export type SelectionCorrectionBubbleState = 'actions' | 'recording' | 'processing' | 'error';

export interface SelectionCorrectionBubblePayload {
  selectedText: string;
  state: SelectionCorrectionBubbleState;
  action: SelectionCorrectionAction | null;
  message: string | null;
}

export function getSelectionCorrection(): Promise<SelectionCorrectionBubblePayload | null> {
  return invokeOrMock('get_selection_correction', undefined, () => ({
    selectedText: 'Codex Context',
    state: 'actions',
    action: null,
    message: null,
  }));
}

export function startSelectionCorrection(action: SelectionCorrectionAction): Promise<void> {
  return invokeOrMock('start_selection_correction', { action }, () => undefined);
}

export function stopSelectionCorrection(): Promise<void> {
  return invokeOrMock('stop_selection_correction', undefined, () => undefined);
}

export function cancelSelectionCorrection(): Promise<void> {
  return invokeOrMock('cancel_selection_correction', undefined, () => undefined);
}

export function dismissSelectionCorrection(): Promise<void> {
  return invokeOrMock('dismiss_selection_correction', undefined, () => undefined);
}
