import { notify } from './notifications';
export type SavedToastEventState = 'saving' | 'saved' | 'deleted' | 'failed';
export function emitSaved(state: SavedToastEventState, message: string, id?: string): void {
  notify({ id, state, message });
}
