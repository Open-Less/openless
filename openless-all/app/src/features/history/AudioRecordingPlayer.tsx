import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { readAudioRecording } from '../../api/history';
import { errorMessage } from './presentation';
import { audioBytes } from './audio';

/** Rendered when session.hasAudioRecording is true: loading is triggered by the detail action
 *  menu, and once bytes arrive it switches to native audio controls. The Blob URL is revoked on
 *  unmount to avoid leaks.
 *  `onMissing` fires when the backend returns 'recording not found' (wav pruned), letting the
 *  parent hide the button for good so the user stops hitting the same error. */
export function AudioRecordingPlayer({
  sessionId,
  onMissing,
  playRequest = 0,
  onLoadingChange,
}: {
  sessionId: string;
  onMissing?: () => void;
  playRequest?: number;
  onLoadingChange?: (loading: boolean) => void;
}) {
  const { t } = useTranslation();
  const [blobUrl, setBlobUrl] = useState<string | null>(null);
  const [status, setStatus] = useState<'idle' | 'loading' | 'ready' | 'error'>('idle');
  const [errorText, setErrorText] = useState<string | null>(null);
  const mountedRef = useRef(true);
  const blobUrlRef = useRef<string | null>(null);
  const initialPlayRequestRef = useRef(playRequest);

  // Revoke the Blob URL on unmount to avoid a memory leak.
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      onLoadingChange?.(false);
      if (blobUrlRef.current) {
        URL.revokeObjectURL(blobUrlRef.current);
        blobUrlRef.current = null;
      }
    };
  }, [onLoadingChange]);

  const clearBlobUrl = () => {
    if (blobUrlRef.current) {
      URL.revokeObjectURL(blobUrlRef.current);
      blobUrlRef.current = null;
    }
    setBlobUrl(null);
  };

  const load = useCallback(async () => {
    setStatus('loading');
    setErrorText(null);
    onLoadingChange?.(true);
    try {
      const dataUrl = await readAudioRecording(sessionId);
      if (!mountedRef.current) return;
      // A Blob URL works in WebKit WebViews where data URLs can decode with zero duration.
      const blob = new Blob([audioBytes(dataUrl)], { type: 'audio/wav' });
      const url = URL.createObjectURL(blob);
      if (!mountedRef.current) {
        URL.revokeObjectURL(url);
        return;
      }
      if (blobUrlRef.current) URL.revokeObjectURL(blobUrlRef.current);
      blobUrlRef.current = url;
      setBlobUrl(url);
      setStatus('ready');
    } catch (error) {
      if (!mountedRef.current) return;
      console.error('[history] load recording failed', error);
      const msg = errorMessage(error);
      if (msg.includes('recording not found') || msg.includes('not found')) {
        onMissing?.();
        return;
      }
      setStatus('error');
      setErrorText(msg);
    } finally {
      if (mountedRef.current) onLoadingChange?.(false);
    }
  }, [onLoadingChange, onMissing, sessionId]);

  useEffect(() => {
    // A newly mounted player may receive an old global counter while the user
    // switches history entries. Only a request created after this instance
    // mounted is allowed to trigger loading/autoplay.
    if (playRequest <= initialPlayRequestRef.current) return;
    void load();
  }, [load, playRequest]);

  if (status === 'ready' && blobUrl) {
    return (
      <div style={{ marginBottom: 14 }}>
        <audio
          src={blobUrl}
          controls
          preload="auto"
          autoPlay
          style={{ width: '100%' }}
          onError={(e) => {
            if (!mountedRef.current) return;
            const a = e.currentTarget;
            const code = a.error?.code ?? -1;
            const detail = a.error?.message ?? `${code}`;
            console.error('[history] <audio> decode/play failed', { code, detail });
            clearBlobUrl();
            setStatus('error');
            setErrorText(t('history.audioDecodeFailed', { err: detail }));
          }}
        />
      </div>
    );
  }
  return status === 'loading' || status === 'error' ? (
    <div style={{ marginBottom: 14, fontSize: 11, color: 'var(--ol-err)' }}>
      {status === 'loading' ? t('history.audioLoading') : errorText}
    </div>
  ) : null;
}
