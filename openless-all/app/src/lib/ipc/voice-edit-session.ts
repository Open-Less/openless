import { invokeOrMock } from './shared';

export type VoiceEditPhase =
  | 'dictating'
  | 'draft_ready'
  | 'editing'
  | 'applying'
  | 'preview'
  | 'committing'
  | 'completed'
  | 'cancelled';

export interface VoiceEditTurn {
  instructionRaw: string;
  instructionPolished: string;
  editPlan: unknown;
  previewAfter: string;
}

export interface VoiceEditContext {
  fieldText: string;
  selection?: { start: number; end: number } | null;
  dictatedSegment: string;
  draft: string;
  preview: string;
  turns: VoiceEditTurn[];
}

export interface VoiceEditSnapshot {
  sessionId: string;
  phase: VoiceEditPhase;
  context?: VoiceEditContext | null;
  target: { kind: 'selection'; start: number; end: number } | { kind: 'full_field' };
}

const mockSnapshot = (phase: VoiceEditPhase = 'dictating'): VoiceEditSnapshot => ({
  sessionId: 'voice-edit-demo',
  phase,
  context: {
    fieldText: '',
    selection: null,
    dictatedSegment: '',
    draft: '',
    preview: '',
    turns: [],
  },
  target: { kind: 'full_field' },
});

export function startVoiceEditSession(): Promise<VoiceEditSnapshot> {
  return invokeOrMock('start_voice_edit_session', undefined, () => mockSnapshot());
}

export function openVoiceEditWindow(): Promise<void> {
  return invokeOrMock('voice_edit_window_open', undefined, () => undefined);
}

export function closeVoiceEditWindow(sessionId?: string): Promise<void> {
  return invokeOrMock('voice_edit_window_close', { sessionId }, () => undefined);
}

export function finalizeVoiceEditDictation(sessionId: string): Promise<VoiceEditSnapshot> {
  return invokeOrMock('finalize_voice_edit_dictation', { sessionId }, () =>
    mockSnapshot('draft_ready'),
  );
}

export function startVoiceEditInstruction(sessionId: string): Promise<VoiceEditSnapshot> {
  return invokeOrMock('start_voice_edit_instruction', { sessionId }, () => mockSnapshot('editing'));
}

export function finalizeVoiceEditInstruction(sessionId: string): Promise<VoiceEditSnapshot> {
  return invokeOrMock('stop_voice_edit_instruction', { sessionId }, () => mockSnapshot('preview'));
}

export function commitVoiceEditSession(sessionId: string): Promise<VoiceEditSnapshot> {
  return invokeOrMock('commit_voice_edit', { sessionId }, () => mockSnapshot('completed'));
}

export function cancelVoiceEditSession(sessionId?: string): Promise<VoiceEditSnapshot | null> {
  return invokeOrMock('cancel_voice_edit_session', { sessionId }, () => null);
}

export async function cancelAndCloseVoiceEditSession(sessionId?: string): Promise<void> {
  const currentId = sessionId ?? (await getVoiceEditState())?.sessionId;
  await cancelVoiceEditSession(currentId);
  await closeVoiceEditWindow(currentId);
}

export function getVoiceEditState(): Promise<VoiceEditSnapshot | null> {
  return invokeOrMock('get_voice_edit_state', undefined, () => null);
}
