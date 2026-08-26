import type { RemoteInputStatus } from '../../lib/ipc'

export type RemoteInputViewState =
  | 'disabled'
  | 'loading'
  | 'starting'
  | 'running'
  | 'stale'
  | 'waiting'
  | 'error'

export function getRemoteInputViewState(
  enabled: boolean,
  status: RemoteInputStatus | null,
  startError: { reason: string; port: number } | null,
): RemoteInputViewState {
  if (!enabled) return 'disabled'
  if (startError != null) return 'error'
  if (status == null) return 'loading'
  if (status.starting) return 'starting'
  if (status.running) return status.urlsStale ? 'stale' : 'running'
  return 'waiting'
}
