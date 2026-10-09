import type { ProviderDescriptor, ProviderKind } from './ipc';
import type { CredentialsStatus, UserPreferences } from './types';

export interface OverviewServiceDetails {
  model: string | null;
  error: boolean;
}

interface ServiceReader {
  listProviderDescriptors: (kind: ProviderKind) => Promise<ProviderDescriptor[]>;
  readCredential: (account: string) => Promise<string | null>;
}

function localModel(providerType: string, prefs: UserPreferences | null): string | undefined {
  switch (providerType) {
    case 'local-qwen3':
    case 'local-qwen3-c':
    case 'local-qwen3-mlx':
      return prefs?.localAsrActiveModel ?? '';
    case 'local-whisper':
      return prefs?.localWhisperActiveModel ?? '';
    case 'foundry-local-whisper':
      return prefs?.foundryLocalAsrModel ?? '';
    case 'sherpa-onnx-local':
      return prefs?.sherpaOnnxModel ?? '';
    case 'apple-speech':
      return '';
    default:
      return undefined;
  }
}

/** Read only the current channel's model field. Omitting the channel id lets the
 * backend resolve the active account, including multiple accounts of one vendor. */
export async function loadOverviewServiceDetails(
  credentials: CredentialsStatus,
  prefs: UserPreferences | null,
  reader: ServiceReader,
): Promise<Partial<Record<ProviderKind, OverviewServiceDetails>>> {
  const kinds: ProviderKind[] =
    credentials.pipelineMode === 'multimodal' ? ['omni'] : ['asr', 'llm'];
  const entries = await Promise.all(
    kinds.map(async (kind) => {
      const providerType =
        kind === 'omni'
          ? (prefs?.activeOmniProvider ?? null)
          : kind === 'asr'
            ? credentials.activeAsrProvider
            : credentials.activeLlmProvider;
      try {
        const local = kind === 'asr' ? localModel(providerType ?? '', prefs) : undefined;
        let model: string | null;
        if (local !== undefined) {
          model = local.trim() || null;
        } else {
          const account = { asr: 'asr.model', llm: 'ark.model_id', omni: 'omni.model' }[kind];
          const [storedModel, descriptors] = await Promise.all([
            reader.readCredential(account),
            reader.listProviderDescriptors(kind),
          ]);
          model =
            storedModel?.trim() ||
            descriptors.find((descriptor) => descriptor.providerType === providerType)
              ?.defaultModel ||
            null;
        }
        return [kind, { model, error: false }] as const;
      } catch {
        return [kind, { model: null, error: true }] as const;
      }
    }),
  );
  return Object.fromEntries(entries);
}
