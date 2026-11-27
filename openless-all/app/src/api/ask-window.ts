import { invokeOrPreview } from '../runtime/transport';

export function openAskWindow(): Promise<void> {
  return invokeOrPreview('ask_window_open', undefined, () => undefined);
}
