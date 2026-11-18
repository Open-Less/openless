import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { CredentialsStatus } from '../../api/contracts';
import {
  getCredentials,
  readCredential,
  listProviderDescriptors,
  listenCredentialsChanges,
  type ProviderKind,
} from '../../api/providers';
import {
  loadOverviewServiceDetails,
  type OverviewServiceDetails,
} from '../../lib/overviewServices';
import { getOverviewSetup, type OverviewSettingsSection } from '../../lib/overviewSetup';
import { useHotkeySettings } from '../../data/preferences';
import { isDesktop } from '../../runtime/platform';
import { Button } from '../../ui/controls/Button';

type RefreshOutcome = 'ok' | 'stale' | 'error';
type Details = Partial<Record<ProviderKind, OverviewServiceDetails>>;
type ServiceState = {
  creds: CredentialsStatus | null;
  serviceDetails: Details;
  loading: boolean;
  error: boolean;
};

export function useOverviewServices() {
  const { prefs, capability } = useHotkeySettings();
  const preferences = useRef(prefs);
  preferences.current = prefs;
  const request = useRef(0);
  const [state, setState] = useState<ServiceState>({
    creds: null,
    serviceDetails: {},
    loading: true,
    error: false,
  });

  const refresh = useCallback(async (): Promise<RefreshOutcome> => {
    const id = ++request.current;
    try {
      const status = await getCredentials();
      if (request.current !== id) return 'stale';
      const details = await loadOverviewServiceDetails(status, preferences.current, {
        listProviderDescriptors,
        readCredential,
      });
      if (request.current !== id) return 'stale';
      setState({ creds: status, serviceDetails: details, loading: false, error: false });
      return 'ok';
    } catch (error) {
      if (request.current !== id) return 'stale';
      console.error('[overview] could not read service status', error);
      setState((current) => ({ ...current, loading: false, error: true }));
      return 'error';
    }
  }, []);

  useEffect(() => {
    let disposed = false;
    let stop: (() => void) | undefined;
    void listenCredentialsChanges(() => {
      if (!disposed) void refresh();
    })
      .then(
        (unlisten) => {
          if (disposed) unlisten();
          else stop = unlisten;
        },
        (error: unknown) =>
          console.warn('[overview] could not subscribe to credentials changes', error),
      )
      .finally(() => {
        if (!disposed) void refresh();
      });
    return () => {
      disposed = true;
      request.current++;
      stop?.();
    };
  }, [
    refresh,
    prefs?.activeLlmProvider,
    prefs?.activeAsrProvider,
    prefs?.pipelineMode,
    prefs?.activeOmniProvider,
    prefs?.localAsrActiveModel,
    prefs?.localWhisperActiveModel,
    prefs?.foundryLocalAsrModel,
    prefs?.sherpaOnnxModel,
  ]);

  const setup = getOverviewSetup({
    credentials: state.creds,
    loading: state.loading,
    error: state.error,
    omniProvider: prefs?.activeOmniProvider,
    desktop: isDesktop(),
    hotkeyAvailable: prefs && capability ? capability.adapter !== 'unavailable' : null,
    hasShortcut: Boolean(prefs?.dictationHotkey.primary.trim()),
  });
  return { ...state, setup, refresh };
}

export function OverviewServiceStatus({
  services,
  onOpenSettings,
}: {
  services: ReturnType<typeof useOverviewServices>;
  onOpenSettings?: (section: OverviewSettingsSection, serviceView?: ProviderKind) => void;
}) {
  const { t } = useTranslation();
  if (services.setup.providers.length === 0) {
    return (
      <span role="status" className="ol-overview-service-status">
        {t(services.loading ? 'overview.statusLoading' : 'overview.credentialsLoadError')}
      </span>
    );
  }
  return (
    <div className="ol-overview-services">
      {services.setup.providers.map((provider) => {
        const { kind, configured, id } = provider;
        const detail = services.serviceDetails[kind];
        const failed = services.error || detail?.error;
        const status = t(
          failed
            ? 'settings.providers.readFailed'
            : configured
              ? 'overview.statusConfigured'
              : 'overview.statusNotConfigured',
        );
        const summary = [status, id, detail?.model].filter(Boolean).join(' · ');
        return (
          <Button
            key={kind}
            title={summary}
            className="ol-overview-service"
            icon={kind === 'asr' ? 'mic' : 'sparkle'}
            size="sm"
            disabled={!onOpenSettings}
            onClick={() => onOpenSettings?.('services', kind)}
            ariaLabel={`${t(`overview.${kind}Kind`)} · ${summary}`}
          >
            <span
              className="ol-overview-service-marker"
              data-status={failed ? 'failed' : configured ? 'ready' : 'missing'}
            />
            {t(`overview.${kind}Kind`)}
          </Button>
        );
      })}
    </div>
  );
}
