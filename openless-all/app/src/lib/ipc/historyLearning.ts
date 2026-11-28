import { invokeOrMock } from './shared';

export interface HistoryLearningSource {
  token: string;
  historyId: string;
  originalText: string;
  expiresAtMs: number;
}
export interface HistoryLearningCandidate { id: string; pattern: string; replacement: string }
export interface HistoryLearningResult {
  candidates: HistoryLearningCandidate[];
  expressionRecorded: boolean;
}
export type HistoryLearningErrorKey = 'learningNotEligible' | 'learningSourceExpired'
  | 'learningPermissionOff' | 'learningPayloadConflict' | 'learningEmpty'
  | 'learningLimit' | 'learningStoreFailed' | 'learningWindowDenied';

// Preview deliberately supplies synthetic data, never an external observation.
const preview = new Map<string, { source: HistoryLearningSource; request?: string; result?: HistoryLearningResult; accepted?: Set<string> }>();
export function beginHistoryLearning(historyId: string): Promise<HistoryLearningSource> {
  return invokeOrMock('begin_history_learning', { historyId }, () => {
    const source = { token: crypto.randomUUID(), historyId, originalText: '请使用 codx 完成这个任务', expiresAtMs: Date.now() + 300_000 };
    preview.set(source.token, { source });
    return source;
  });
}
export function submitHistoryLearning(token: string, correctedText: string, requestVocabulary: boolean, requestExpression: boolean): Promise<HistoryLearningResult> {
  return invokeOrMock('submit_history_learning', { token, correctedText, requestVocabulary, requestExpression }, () => {
    const item = preview.get(token);
    if (!item || item.source.expiresAtMs <= Date.now()) throw new Error('learningSourceExpired');
    const request = JSON.stringify([correctedText, requestVocabulary, requestExpression]);
    if (item.request && item.request !== request) throw new Error('learningPayloadConflict');
    item.request = request;
    return item.result ??= { candidates: requestVocabulary && correctedText === '请使用 codex 完成这个任务'
      ? [{ id: crypto.randomUUID(), pattern: 'codx', replacement: 'codex' }] : [], expressionRecorded: false };
  });
}
export function acceptHistoryLearning(token: string, candidateId: string): Promise<boolean> {
  return invokeOrMock('accept_history_learning', { token, candidateId }, () => {
    const item = preview.get(token);
    if (!item || item.source.expiresAtMs <= Date.now()) throw new Error('learningSourceExpired');
    const candidate = item.result?.candidates.find(c => c.id === candidateId);
    if (item.accepted?.has(candidateId)) return false;
    if (!candidate) throw new Error('learningSourceExpired');
    (item.accepted ??= new Set()).add(candidateId);
    return true;
  });
}
export function cancelHistoryLearning(token: string): Promise<void> {
  return invokeOrMock('cancel_history_learning', { token }, () => { preview.delete(token); });
}
