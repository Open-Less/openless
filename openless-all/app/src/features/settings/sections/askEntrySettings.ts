import { invokeOrPreview } from '../../../runtime/transport';

export type AskEntryIntentMode = 'semanticHybrid' | 'separateModel' | 'mainModelTool';

export interface AskEntrySettings {
  autoVoiceOnOpen: boolean;
  intentMode: AskEntryIntentMode;
}

function nativeOnly(): never {
  throw new Error('Ask entry settings require the native main window.');
}

let pendingOperation: Promise<void> = Promise.resolve();

function serialize<T>(operation: () => Promise<T>): Promise<T> {
  const result = pendingOperation.then(operation);
  pendingOperation = result.then(
    () => undefined,
    () => undefined,
  );
  return result;
}

export function readAskEntrySettings(): Promise<AskEntrySettings> {
  return serialize(() => invokeOrPreview('ask_get_entry_settings', undefined, nativeOnly));
}

export function writeAskEntrySettings(settings: Partial<AskEntrySettings>): Promise<AskEntrySettings> {
  return serialize(() => invokeOrPreview('ask_set_entry_settings', { settings }, nativeOnly));
}
