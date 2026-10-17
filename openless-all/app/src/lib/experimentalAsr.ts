import { invokeOrMock } from './ipc';
import { createChannel, deleteChannel, listChannels, setChannelEnabled } from './ipc/channels';

export type ExperimentalAsrModel = 'text-stream' | 'sense-voice' | 'embedding-gemma';
export type ExperimentalAsrPhase =
  'disabled' | 'downloading' | 'preparing' | 'ready' | 'removing' | 'failed';
export interface ExperimentalAsrStatus {
  model: ExperimentalAsrModel;
  enabled: boolean;
  phase: ExperimentalAsrPhase;
  available: boolean;
  unavailableReason: string | null;
  downloadedBytes: number;
  totalBytes: number;
  channelId: string | null;
  needsGuidance: boolean;
  error: string | null;
}
export const EXPERIMENTAL_ASR_MODELS = ['text-stream', 'sense-voice', 'embedding-gemma'] as const;
export function experimentalAsrModelFromDownloadId(id: string): ExperimentalAsrModel | null {
  return (
    (
      {
        'experimental-textstream-qwen3': 'text-stream',
        'experimental-sensevoice-small': 'sense-voice',
        'experimental-embedding-gemma-en': 'embedding-gemma',
      } as Record<string, ExperimentalAsrModel>
    )[id] ?? null
  );
}
export function experimentalAsrProvider(model: ExperimentalAsrModel): string {
  return {
    'text-stream': 'experimental-textstream',
    'sense-voice': 'experimental-sensevoice',
    'embedding-gemma': 'experimental-embedding-gemma',
  }[model];
}
export function isRetiredMacAsrProvider(provider: string): boolean {
  return [
    'local-qwen3',
    'local-qwen3-mlx',
    'local-qwen3-c',
    'local-whisper',
    'apple-whisper',
  ].includes(provider);
}
export function isExperimentalAsrProvider(provider: string): boolean {
  return EXPERIMENTAL_ASR_MODELS.some((model) => experimentalAsrProvider(model) === provider);
}
export function experimentalAsrName(model: ExperimentalAsrModel): string {
  return {
    'text-stream': 'TextStream',
    'sense-voice': 'SenseVoice Small',
    'embedding-gemma': 'Embedding Gemma',
  }[model];
}
export function downloadPercent(status: ExperimentalAsrStatus): number {
  return Math.min(
    100,
    Math.max(0, Math.floor((status.downloadedBytes / Math.max(1, status.totalBytes)) * 100)),
  );
}
export function needsModelGuidance(status: ExperimentalAsrStatus): boolean {
  return (
    status.enabled && status.phase === 'ready' && status.channelId !== null && status.needsGuidance
  );
}
const mock = EXPERIMENTAL_ASR_MODELS.map((model): ExperimentalAsrStatus => ({
  model,
  enabled: false,
  available: true,
  unavailableReason: null,
  phase: 'disabled',
  downloadedBytes: 0,
  totalBytes:
    model === 'text-stream' ? 1880560703 : model === 'sense-voice' ? 239549735 : 714975724,
  channelId: null,
  needsGuidance: false,
  error: null,
}));
const mockStart = new Map<ExperimentalAsrModel, number>();
export async function getExperimentalAsrStatus(): Promise<ExperimentalAsrStatus[]> {
  const result = await invokeOrMock('experimental_asr_status', {}, async () => {
    for (const status of mock) {
      if (status.phase !== 'downloading' && status.phase !== 'preparing') continue;
      const elapsed = Date.now() - (mockStart.get(status.model) ?? Date.now());
      status.downloadedBytes = Math.floor(status.totalBytes * Math.min(1, elapsed / 1800));
      if (elapsed >= 1800) {
        if (!status.channelId) {
          const hasSelection = (await listChannels('asr')).some((channel) => channel.enabled);
          status.channelId = await createChannel(
            'asr',
            experimentalAsrProvider(status.model),
            experimentalAsrName(status.model),
          );
          if (!hasSelection) await setChannelEnabled('asr', status.channelId, false);
        }
        status.phase = 'ready';
        status.needsGuidance = true;
      }
    }
    return mock.map((status) => ({ ...status }));
  });
  return [...result].sort(
    (a, b) => EXPERIMENTAL_ASR_MODELS.indexOf(a.model) - EXPERIMENTAL_ASR_MODELS.indexOf(b.model),
  );
}
export async function setExperimentalAsrEnabled(
  model: ExperimentalAsrModel,
  enabled: boolean,
): Promise<void> {
  return invokeOrMock('experimental_asr_set_enabled', { model, enabled }, async () => {
    const status = mock.find((s) => s.model === model)!;
    if (status.enabled === enabled && status.phase !== 'failed') return;
    status.enabled = enabled;
    status.error = null;
    status.phase = enabled ? 'downloading' : 'disabled';
    if (enabled) mockStart.set(model, Date.now());
    else {
      if (status.channelId) await deleteChannel('asr', status.channelId);
      status.channelId = null;
      status.needsGuidance = false;
      mockStart.delete(model);
    }
  });
}
export function deleteExperimentalAsr(model: ExperimentalAsrModel): Promise<void> {
  return invokeOrMock('experimental_asr_delete', { model }, () => {
    const status = mock.find((s) => s.model === model)!;
    if (status.enabled) throw new Error('Disable the model before deleting its files');
    status.downloadedBytes = 0;
  });
}
export function acknowledgeExperimentalAsr(model: ExperimentalAsrModel): Promise<void> {
  return invokeOrMock('experimental_asr_acknowledge', { model }, () => {
    mock.find((s) => s.model === model)!.needsGuidance = false;
  });
}
