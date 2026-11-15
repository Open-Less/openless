import { dismissNotification, notify } from './notifications';
export type SavedToastEventState = 'saving' | 'saved' | 'deleted' | 'failed';
export function emitSaved(state: SavedToastEventState, message: string, id?: string): void {
  if (state !== 'saving' && id) dismissNotification(id);
  notify({ id: state === 'saving' ? id : undefined, state, message });
}
