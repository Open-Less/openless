export const bailianProtocols = [
  'auto',
  'dashscope-realtime',
  'qwen-realtime',
  'multimodal',
  'qwen-multimodal',
  'async-transcription',
] as const;
export type BailianProtocol = (typeof bailianProtocols)[number];

export function readBailianProtocol(raw: string | null | undefined): BailianProtocol {
  const value: unknown = raw?.trim() ? JSON.parse(raw) : {};
  if (!value || typeof value !== 'object' || Array.isArray(value))
    throw new Error('Invalid ASR configuration');
  const protocol = (value as Record<string, unknown>).bailianProtocol ?? 'auto';
  if (!bailianProtocols.includes(protocol as BailianProtocol))
    throw new Error('Invalid Bailian protocol');
  return protocol as BailianProtocol;
}

export function writeBailianProtocol(
  raw: string | null | undefined,
  protocol: BailianProtocol,
): string {
  readBailianProtocol(raw);
  const value = raw?.trim() ? JSON.parse(raw) : {};
  if (protocol === 'auto') delete value.bailianProtocol;
  else value.bailianProtocol = protocol;
  return JSON.stringify(value);
}
