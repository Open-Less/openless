import { useCallback, useEffect, useRef, useState } from 'react';
import {
  checkMicrophonePermission,
  openSystemSettings,
  requestMicrophonePermission,
} from '../../api/permissions';
import type { PermissionStatus } from '../../api/contracts';
import { getPlatformCapabilities } from '../../runtime/platform';
import {
  checkAndroidMicrophoneAccess,
  requestAndroidMicrophoneAccess,
} from '@android/lib/androidMicrophonePermission';

/** Entry reads are passive. Only an explicit action can request access or open Settings. */
export function useMicrophoneAccess(enabled: boolean) {
  const [status, setStatus] = useState<PermissionStatus | 'loading'>('loading');
  const [error, setError] = useState<string | null>(null);
  const revision = useRef(0);
  const recheckOnReturn = useRef(false);

  const read = useCallback(async (requestAccess = false) => {
    const current = ++revision.current;
    setError(null);
    setStatus('loading');
    try {
      // A cold settings entry can mount before the native capability handshake finishes.
      const caps = await getPlatformCapabilities();
      if (current !== revision.current) return;
      const android = caps.platform === 'android';
      const next = requestAccess
        ? await (android ? requestAndroidMicrophoneAccess() : requestMicrophonePermission())
        : await (android ? checkAndroidMicrophoneAccess() : checkMicrophonePermission());
      if (current !== revision.current) return;
      setStatus(next);
      if (requestAccess && (next === 'denied' || next === 'restricted')) {
        recheckOnReturn.current = true;
        await openSystemSettings('microphone');
      }
    } catch (cause) {
      if (current === revision.current) {
        recheckOnReturn.current = false;
        setError(String(cause));
      }
    }
  }, []);

  useEffect(() => {
    if (!enabled) return;
    void read();
    const onFocus = () => {
      if (!recheckOnReturn.current) return;
      recheckOnReturn.current = false;
      void read();
    };
    const onVisibility = () => {
      if (document.visibilityState === 'visible') onFocus();
    };
    window.addEventListener('focus', onFocus);
    document.addEventListener('visibilitychange', onVisibility);
    return () => {
      revision.current++;
      recheckOnReturn.current = false;
      window.removeEventListener('focus', onFocus);
      document.removeEventListener('visibilitychange', onVisibility);
    };
  }, [enabled, read]);

  const request = async () => {
    if (status === 'denied' || status === 'restricted') {
      recheckOnReturn.current = true;
      try {
        await openSystemSettings('microphone');
      } catch (cause) {
        recheckOnReturn.current = false;
        setError(String(cause));
      }
    } else {
      await read(true);
    }
  };
  return { status, error, request, refresh: () => read() };
}
