import type {
  FoundryPrepareProgress,
  LocalAsrDownloadProgress,
  SherpaPrepareProgress,
} from '../api/local-asr';
import { listenTauriEvent } from '../runtime/transport';

type Unlisten = () => void;
type Observe<T> = (listener: (payload: T) => void) => Promise<Unlisten>;

export function shareProgressEvent<T>(observe: Observe<T>): Observe<T> {
  const subscribers = new Set<(payload: T) => void>();
  let connection: Promise<Unlisten> | undefined;
  return async (handler) => {
    const receive = (payload: T) => handler(payload);
    subscribers.add(receive);
    const registration = (connection ??= observe((payload) => {
      for (const subscriber of subscribers) subscriber(payload);
    }));
    try {
      const stop = await registration;
      return () => {
        if (!subscribers.delete(receive)) return;
        if (subscribers.size === 0 && connection === registration) {
          connection = undefined;
          stop();
        }
      };
    } catch (error) {
      subscribers.delete(receive);
      if (connection === registration) connection = undefined;
      throw error;
    }
  };
}

interface ModelProgressEvents {
  'local-asr-download-progress': LocalAsrDownloadProgress;
  'sherpa-onnx-asr-download-progress': LocalAsrDownloadProgress;
  'sherpa-onnx-asr-prepare-progress': SherpaPrepareProgress;
  'foundry-local-asr-prepare-progress': FoundryPrepareProgress;
}

function modelProgressEvent<K extends keyof ModelProgressEvents>(event: K) {
  return shareProgressEvent<ModelProgressEvents[K]>((handler) => listenTauriEvent(event, handler));
}

const events: { [K in keyof ModelProgressEvents]: Observe<ModelProgressEvents[K]> } = {
  'local-asr-download-progress': modelProgressEvent('local-asr-download-progress'),
  'sherpa-onnx-asr-download-progress': modelProgressEvent('sherpa-onnx-asr-download-progress'),
  'sherpa-onnx-asr-prepare-progress': modelProgressEvent('sherpa-onnx-asr-prepare-progress'),
  'foundry-local-asr-prepare-progress': modelProgressEvent('foundry-local-asr-prepare-progress'),
};

export function listenModelProgress<K extends keyof ModelProgressEvents>(
  event: K,
  handler: (payload: ModelProgressEvents[K]) => void,
): Promise<Unlisten> {
  return events[event](handler);
}
