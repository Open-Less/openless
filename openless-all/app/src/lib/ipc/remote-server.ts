import { invokeOrMock } from './shared';

// ── Remote input (LAN phone recording) ──────────────────────────────
export interface RemoteInputStatus {
  running: boolean;
  starting: boolean;
  port: number;
  pin: string;
  urls: string[];
  urlsStale: boolean;
  /** Full SHA-256 of the root CA used by the running service, fetched via local IPC. */
  caFingerprintSha256?: string;
}

export function getRemoteInputStatus(): Promise<RemoteInputStatus> {
  return invokeOrMock('get_remote_input_status', undefined, () => ({
    running: false,
    starting: false,
    port: 8443,
    pin: '000000',
    urls: [],
    urlsStale: false,
  }));
}

export function listLocalIps(): Promise<string[]> {
  return invokeOrMock('list_local_ips', undefined, () => ['192.168.1.100']);
}

export function regenerateRemotePin(): Promise<string> {
  return invokeOrMock('regenerate_remote_pin', undefined, () => '123456');
}

/** Syncs the PC UI language to the remote input service so the H5 recording page displays that language. */
export function setRemoteLocale(locale: string): Promise<void> {
  return invokeOrMock('set_remote_locale', { locale }, () => undefined);
}
