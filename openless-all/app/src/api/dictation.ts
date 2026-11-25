import type {
  BackendEvent,
  CapsulePayload,
  InsertFallbackCardPayload,
  PendingCorrection,
  UserPreferences,
} from './contracts';
import { invokeOrPreview } from '../runtime/transport';
import { createAuxiliaryWindowSubscriptions } from '../runtime/windows/auxiliaryEvents';

export * from '../lib/ipc/dictation';
export { dismissInsertFallbackCard, reportInsertFallbackCardHeight } from '../lib/ipc/vocab';

export type CaptionTraceStage = 'accepted' | 'rejected' | 'render_callback';

export function mainCaptionTraceEnabled() {
  return invokeOrPreview('main_caption_trace_enabled', undefined, () => false);
}

export function reportMainCaptionTrace(sessionId: string, sequence: number, stage: CaptionTraceStage) {
  return invokeOrPreview<void>('report_main_caption_trace', { sessionId, sequence, stage }, () => {});
}

export function getCapsuleAuxiliarySnapshot() {
  return invokeOrPreview<{
    suggestions: PendingCorrection[];
    fallback: InsertFallbackCardPayload | null;
  }>('get_capsule_auxiliary_snapshot', undefined, () => ({ suggestions: [], fallback: null }));
}

export function createCapsuleSubscriptions() {
  const events = createAuxiliaryWindowSubscriptions();
  return {
    label: events.label,
    dispose: events.dispose,
    state: (onPayload: (payload: CapsulePayload) => void) =>
      events.listen('capsule:state', onPayload, 'window'),
    transcript: (onPayload: (payload: BackendEvent) => void) =>
      events.listen('backend:event', onPayload, 'app'),
    preferences: (onPayload: (payload: UserPreferences) => void) =>
      events.listen('prefs:changed', onPayload, 'app'),
    suggestions: (onPayload: (payload: PendingCorrection[]) => void) =>
      events.listen('vocab:suggested', onPayload, 'window'),
    fallback: (onPayload: (payload: InsertFallbackCardPayload | null) => void) =>
      events.listen('insert:fallback', onPayload, 'window'),
  };
}
