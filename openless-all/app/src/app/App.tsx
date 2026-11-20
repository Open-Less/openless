import { lazy, Suspense, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { TooltipProvider } from '../ui/overlays/Tooltip';
import { installEncryptedSyncUiBridge } from '../lib/encryptedSyncUiBridge';
import { markStartup } from '../lib/startupMetrics';
import { getRuntimeEnvironment, type OS } from '../runtime/environment';
import { listenTauriEvent, requireBackendReady } from '../runtime/transport';
import type { BackendEvent } from '../lib/backendEvent';
import { AppViewport } from '../ui/layout/AppViewport';
import { PreviewFrame } from '../ui/layout/PreviewFrame';
import { InteractionProvider, useInteraction } from '../ui/overlays/InteractionProvider';
import { CoreStartupScreen } from './CoreStartupScreen';
import { MainWindowFirstFrame } from './MainWindowFirstFrame';
import { isCompactWindow, isDormantWindow, WindowRoutes, type WindowKind } from './windowRoutes';

const GlobalDownloadProgress = lazy(() =>
  import('../features/downloads/GlobalDownloadProgress').then((module) => ({
    default: module.GlobalDownloadProgress,
  })),
);

function MainDictationNotifications() {
  const { notify } = useInteraction('main-dictation');
  const { t } = useTranslation();
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listenTauriEvent<BackendEvent>('backend:event', ({ kind }) => {
      if (disposed || kind.type !== 'dictation_state_changed') return;
      const state = kind.payload as { phase?: string } | undefined;
      if (state?.phase === 'failed') {
        notify({ key: 'dictation-failed', state: 'failed', message: t('common.dictationFailed') });
      }
    }).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    }).catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [notify, t]);
  return null;
}

interface AppProps {
  kind: WindowKind;
  os: OS;
  previewOnboarding?: boolean;
  previewFrame?: { width: number; height: number };
  languageError?: string;
}

export function App({
  kind,
  os,
  previewOnboarding = false,
  previewFrame,
  languageError,
}: AppProps) {
  const { host } = getRuntimeEnvironment();
  const [backend, setBackend] = useState<'checking' | 'ready' | { error: string }>(
    host === 'preview' ? 'ready' : 'checking',
  );
  const compact = isCompactWindow(kind);

  useEffect(() => {
    if (host === 'preview' || languageError) return;
    let active = true;
    void requireBackendReady()
      .then(() => {
        if (!active) return;
        markStartup('backend');
        if (kind === 'main' && host === 'tauri') {
          // Listener installation must not hold the first frame behind native storage.
          void installEncryptedSyncUiBridge()
            .then(() => markStartup('sync'))
            .catch(() => {});
        }
        setBackend('ready');
      })
      .catch((reason: unknown) => {
        if (!active) return;
        console.error('[startup] backend contract handshake failed', reason);
        setBackend({ error: reason instanceof Error ? reason.message : String(reason) });
      });
    return () => {
      active = false;
    };
  }, [host, kind, languageError]);

  if (isDormantWindow(kind) && backend === 'ready' && !languageError) return null;

  const error = languageError ?? (typeof backend === 'object' ? backend.error : undefined);
  const renderStartup = error || backend === 'checking';
  const viewport = (
    <AppViewport scale={kind === 'main'}>
      <InteractionProvider
        notificationAside={
          kind === 'main' && !renderStartup ? (
            <>
              {host === 'tauri' && <MainDictationNotifications />}
              <Suspense fallback={null}>
                <GlobalDownloadProgress />
              </Suspense>
            </>
          ) : null
        }
      >
        <TooltipProvider>
          {renderStartup ? (
            <>
              <CoreStartupScreen compact={compact} error={error} />
              {error && kind === 'main' && <MainWindowFirstFrame os={os} />}
            </>
          ) : (
            <Suspense fallback={<CoreStartupScreen compact={compact} />}>
              <WindowRoutes kind={kind} os={os} previewOnboarding={previewOnboarding} />
            </Suspense>
          )}
        </TooltipProvider>
      </InteractionProvider>
    </AppViewport>
  );
  return previewFrame ? <PreviewFrame {...previewFrame}>{viewport}</PreviewFrame> : viewport;
}
