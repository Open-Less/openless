import { invokeOrMock } from './shared';
export interface SiwcAccount {
  id: string;
  label: string;
  email: string | null;
}
export interface SiwcError {
  code: string;
  httpStatus?: number;
  param?: string;
  requestId?: string;
}
export interface SiwcStatus {
  state:
    | 'signed_out'
    | 'authorizing'
    | 'connected'
    | 'usage_disabled'
    | 'usage_limited'
    | 'reauthorization_required';
  account: SiwcAccount | null;
  accounts: SiwcAccount[];
  attemptId: string | null;
  error: SiwcError | null;
  welcomePending: boolean;
}
export interface SiwcModel {
  slug: string;
  displayName: string;
}
export type SiwcAction =
  | 'sign_in'
  | 'add_account'
  | 'enable_usage'
  | 'cancel'
  | 'select'
  | 'disconnect'
  | 'acknowledge'
  | 'retry';
const preview = new Map<string, SiwcStatus>();
const initial = (): SiwcStatus => ({
  state: 'signed_out',
  account: null,
  accounts: [],
  attemptId: null,
  error: null,
  welcomePending: false,
});
export function siwcStatus(channelId: string): Promise<SiwcStatus> {
  return invokeOrMock('siwc_status', { channelId }, () =>
    structuredClone(preview.get(channelId) ?? initial()),
  );
}
export function siwcModels(channelId: string): Promise<SiwcModel[]> {
  return invokeOrMock('siwc_models', { channelId }, () => [
    { slug: 'preview-model', displayName: 'Preview model' },
  ]);
}
export function siwcAction(
  channelId: string,
  action: SiwcAction,
  registrationId?: string,
): Promise<{ attemptId?: string; remoteRevoked?: boolean } | null> {
  return invokeOrMock('siwc_action', { channelId, action, registrationId }, () => {
    const status = preview.get(channelId) ?? initial();
    if (action === 'sign_in' || action === 'add_account' || action === 'enable_usage') {
      status.state = 'authorizing';
      status.attemptId = 'preview-authorization';
    } else if (action === 'cancel') {
      status.state = status.account ? 'connected' : 'signed_out';
      status.attemptId = null;
    } else if (action === 'disconnect') {
      status.state = 'reauthorization_required';
      status.attemptId = null;
    } else if (action === 'acknowledge') {
      status.welcomePending = false;
    } else if (action === 'retry') {
      status.state = 'connected';
      status.error = null;
    } else if (action === 'select') {
      status.account = status.accounts.find((a) => a.id === registrationId) ?? null;
      status.state = status.account ? 'connected' : 'signed_out';
    }
    preview.set(channelId, status);
    if (action === 'sign_in' || action === 'add_account' || action === 'enable_usage') {
      return { attemptId: status.attemptId ?? undefined };
    }
    return action === 'disconnect' ? { remoteRevoked: true } : null;
  });
}
export function siwcErrorCode(value: unknown): string {
  if (typeof value === 'object' && value !== null && 'code' in value) return String(value.code);
  const text = value instanceof Error ? value.message : String(value);
  const at = text.indexOf('siwc:');
  if (at >= 0) {
    try {
      return JSON.parse(text.slice(at + 5)).code || 'request_failed';
    } catch {
      /* Only structured, non-secret codes reach the UI. */
    }
  }
  return 'request_failed';
}
