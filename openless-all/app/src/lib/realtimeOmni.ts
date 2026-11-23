/** Realtime options are channel configuration, never HTTP header entries. */
export const realtimeOptionsAccount = 'omni.realtime_options';
export const realtimeOmniProviders = [
  'openai-realtime',
  'dashscope-realtime',
  'dashscope-realtime-legacy',
] as const;
export const realtimeAuthModes = ['bearer', 'api-key', 'headers'] as const;
export const realtimeReasoningEfforts = [
  'auto',
  'omit',
  'none',
  'minimal',
  'low',
  'medium',
  'high',
  'xhigh',
] as const;
export type RealtimeAuthMode = (typeof realtimeAuthModes)[number];
export type RealtimeReasoningEffort = (typeof realtimeReasoningEfforts)[number];
export interface RealtimeOptions {
  authMode: RealtimeAuthMode;
  reasoningEffort: RealtimeReasoningEffort;
  reasoningFallback: boolean;
}
export const defaultRealtimeOptions: Readonly<RealtimeOptions> = {
  authMode: 'bearer',
  reasoningEffort: 'auto',
  reasoningFallback: true,
};
export function isRealtimeOmniProvider(provider: string): boolean {
  return realtimeOmniProviders.some((id) => id === provider);
}
export function parseRealtimeOptions(raw: string | null): RealtimeOptions {
  if (!raw?.trim()) return { ...defaultRealtimeOptions };
  const invalid = () => new Error('Invalid Realtime options');
  if (raw.length > 8192) throw invalid();
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    throw invalid();
  }
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw invalid();
  const object = value as Record<string, unknown>;
  if (
    Object.keys(object).some(
      (key) => !['authMode', 'reasoningEffort', 'reasoningFallback'].includes(key),
    )
  )
    throw invalid();
  const auth = 'authMode' in object ? object.authMode : defaultRealtimeOptions.authMode;
  const effort =
    'reasoningEffort' in object ? object.reasoningEffort : defaultRealtimeOptions.reasoningEffort;
  const fallback =
    'reasoningFallback' in object
      ? object.reasoningFallback
      : defaultRealtimeOptions.reasoningFallback;
  if (
    !realtimeAuthModes.some((mode) => mode === auth) ||
    !realtimeReasoningEfforts.some((mode) => mode === effort) ||
    typeof fallback !== 'boolean'
  )
    throw invalid();
  return {
    authMode: auth as RealtimeAuthMode,
    reasoningEffort: effort as RealtimeReasoningEffort,
    reasoningFallback: fallback,
  };
}
export function serializeRealtimeOptions(options: RealtimeOptions): string {
  return JSON.stringify(parseRealtimeOptions(JSON.stringify(options)));
}
type ReadCredential = (account: string, provider?: string) => Promise<string | null>;
type WriteCredential = (account: string, value: string, provider?: string) => Promise<void>;
export async function loadRealtimeOptions(
  provider: string,
  read: ReadCredential,
): Promise<RealtimeOptions> {
  if (!isRealtimeOmniProvider(provider)) throw new Error('Unsupported Realtime profile');
  return parseRealtimeOptions(await read(realtimeOptionsAccount, provider));
}
export async function saveRealtimeOptions(
  provider: string,
  options: RealtimeOptions,
  write: WriteCredential,
): Promise<void> {
  if (!isRealtimeOmniProvider(provider)) throw new Error('Unsupported Realtime profile');
  await write(realtimeOptionsAccount, serializeRealtimeOptions(options), provider);
}
/** A return visit to a preset must preserve a user's gateway/model and other credentials. */
export async function fillEmptyOmniDefaults(
  provider: string,
  defaults: { baseUrl: string; modelPlaceholder: string },
  read: ReadCredential,
  write: WriteCredential,
): Promise<void> {
  const [endpoint, model] = await Promise.all([
    read('omni.endpoint', provider),
    read('omni.model', provider),
  ]);
  if (!endpoint?.trim() && defaults.baseUrl)
    await write('omni.endpoint', defaults.baseUrl, provider);
  if (!model?.trim() && defaults.modelPlaceholder)
    await write('omni.model', defaults.modelPlaceholder, provider);
}
