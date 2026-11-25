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
import { isPreviewHost } from '../../runtime/environment';
import { readOverviewStatusCache, rememberOverviewStatus } from '../../lib/overviewStatusCache';
import { Icon } from '../../components/Icon';
import { Tooltip } from '../../ui/overlays/Tooltip';

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
  const [state, setState] = useState<ServiceState>(() => {
    const creds = isPreviewHost() ? null : (readOverviewStatusCache()?.creds ?? null);
    return { creds, serviceDetails: {}, loading: creds === null, error: false };
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
      if (!isPreviewHost()) rememberOverviewStatus({ creds: status });
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
  return (
    <div className="ol-overview-services">
      {services.setup.providers.map(({ kind, configured }) => {
        const failed = services.error || services.serviceDetails[kind]?.error;
        const status = failed ? 'failed' : configured ? 'configured' : 'missing';
        const statusLabel = t(
          failed
            ? 'overview.statusFailed'
            : configured
              ? 'overview.statusConfigured'
              : 'overview.statusNotConfigured',
        );
        return (
          <div key={kind} className="ol-overview-service" data-status={status}>
            <Tooltip
              content={t(`overview.serviceHint.${kind}`)}
              placement="bottom"
              className="ol-overview-service-icon"
              trigger={
                <button
                  type="button"
                  disabled={!onOpenSettings}
                  aria-label={`${t(`overview.${kind}Kind`)} ${statusLabel}`}
                  onClick={() => onOpenSettings?.('services', kind)}
                />
              }
            >
              <Icon name={kind === 'asr' ? 'mic' : kind === 'llm' ? 'text' : 'sparkle'} size={16} />
              <span className="ol-overview-service-badge" />
            </Tooltip>
          </div>
        );
      })}
    </div>
  );
}
