// Content page for each sidebar tab in the settings dialog. Each tab is a vertical stack of section cards;
// the real logic lives in the *Section files — this only decides which sections belong to which tab.

import { useTranslation } from 'react-i18next';
import { useEffect, useRef, useState } from 'react';
import { AndroidPermissionsPanel } from '@android/components/AndroidPermissionsPanel';
import { Icon } from '../../components/Icon';
import { RecordingInputSection } from './RecordingInputSection';
import { RemoteInputSection } from './RemoteInputSection';
import { ShortcutsSection } from './ShortcutsSection';
import { SelectionWorkspaceSection } from './SelectionWorkspaceSection';
import { LanguageSection } from './LanguageSection';
import { ThemeSection } from './ThemeSection';
import { ProvidersSection } from './ChannelList';
import { OmniChannelSection } from './ProvidersSection';
import { NetworkSection } from './NetworkSection';
import { MarketplaceSection } from './MarketplaceSection';
import { PermissionsSection } from './PermissionsSection';
import { DataStorageSection } from './DataStorageSection';
import { CloudSyncSection } from './CloudSyncSection';
import { LocalModelsSection } from './models/LocalModelsSection';
import { LocalModelsNavContext } from './models/modelsNav';
import { DebugToolsSection } from './DebugToolsSection';
import { VocabularyLearningSection } from './VocabularyLearningSection';
import { CodingAgentSection } from './CodingAgentSection';
import { ClaudeConsoleSection } from './ClaudeConsoleSection';
import { BetaChannelSection } from './BetaChannelSection';
import { AutoUpdateSection } from './AutoUpdateSection';
import { AboutSection } from './AboutSection';
import { getPlatformCapabilities } from '../../lib/platform';
import { listChannels } from '../../lib/ipc';
import type { PlatformCapabilities } from '../../lib/types';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { emitSaved } from '../../lib/savedEvent';
import { SettingRow, segmentedTrackStyle } from './shared';
import { Card } from '../_atoms';
import { useContentMotion, useSelectionMotion } from '../../lib/motion';
import {
  availableServiceViews,
  isServiceViewInactive,
  resolveServiceView,
  type ServiceViewId,
  type AdvancedPage,
  type AdvancedPageId,
} from './navigation';

// Platform capability query shared by all tabs (drives desktop/mobile and hotkey / auto-update gating).
function usePlatformCaps(): PlatformCapabilities | null {
  const [platformCaps, setPlatformCaps] = useState<PlatformCapabilities | null>(null);

  useEffect(() => {
    void getPlatformCapabilities().then(setPlatformCaps);
  }, []);

  return platformCaps;
}

// Recording & input: from recording to text insertion, plus phone input.
export function GeneralTab() {
  const platformCaps = usePlatformCaps();
  const showRemoteInput = platformCaps?.platform === 'desktop';

  return (
    <>
      <RecordingInputSection />
      {showRemoteInput && <RemoteInputSection />}
    </>
  );
}

export function InputMethodTab() {
  const { t } = useTranslation();
  return (
    <>
      <Card>
        <AndroidPermissionsPanel mode="overlayConfig" />
      </Card>
      <Card>
        <div style={{ display: 'grid', gap: 8 }}>
          <strong style={{ fontSize: 13, color: 'var(--ol-ink)' }}>
            {t('settings.inputMethod.guidanceTitle')}
          </strong>
          <p style={{ margin: 0, fontSize: 12, color: 'var(--ol-ink-3)', lineHeight: 1.6 }}>
            {t('settings.inputMethod.enableSystemIme')}
          </p>
          <p style={{ margin: 0, fontSize: 12, color: 'var(--ol-ink-3)', lineHeight: 1.6 }}>
            {t('settings.inputMethod.longPressLogo')}
          </p>
        </div>
      </Card>
    </>
  );
}

export function ShortcutsTab() {
  const platformCaps = usePlatformCaps();
  if (!platformCaps?.supportsDesktopHotkey) return null;
  return (
    <>
      <ShortcutsSection />
      <SelectionWorkspaceSection />
    </>
  );
}

export function AppearanceTab() {
  return (
    <>
      <ThemeSection />
      <LanguageSection />
    </>
  );
}

// AI services & models: organizes channels, local models, and network settings by current pipeline and platform capabilities.
export function ServicesTab() {
  const { t } = useTranslation();
  const { prefs, updatePrefs } = useHotkeySettings();
  const platformCaps = usePlatformCaps();
  const showLocalModel = platformCaps?.supportsLocalAsr === true;
  const multimodal =
    prefs?.multimodalPipelineEnabled === true && prefs.pipelineMode === 'multimodal';
  const [view, setView] = useState<ServiceViewId>('llm');
  const views = availableServiceViews(showLocalModel);
  const selectedView = resolveServiceView(view, views, multimodal);
  const contentRef = useRef<HTMLDivElement>(null);
  useContentMotion(contentRef, selectedView);
  const pipelineRef = useRef<HTMLDivElement>(null);
  const pipelineIndicatorRef = useRef<HTMLSpanElement>(null);
  const navigationRef = useRef<HTMLDivElement>(null);
  const navigationIndicatorRef = useRef<HTMLSpanElement>(null);
  useSelectionMotion(pipelineRef, pipelineIndicatorRef, multimodal ? 'multimodal' : 'traditional');
  useSelectionMotion(
    navigationRef,
    navigationIndicatorRef,
    `${selectedView}:${views.length}`,
    true,
  );
  const setPipelineMode = (mode: 'traditional' | 'multimodal') => {
    if (!prefs || (multimodal ? 'multimodal' : 'traditional') === mode) return;
    void updatePrefs((current) => ({
      ...current,
      pipelineMode: mode,
      multimodalPipelineEnabled: true,
    })).catch((error) => {
      console.error('[settings] failed to update pipeline mode', error);
      emitSaved('failed', t('common.operationFailed'));
    });
  };

  // Language models / speech recognition are required config: the active tabs carry a status dot — red when
  // unconfigured, green when configured. After any channel add/remove/edit/toggle ChannelList broadcasts
  // ol-channels-changed and this recomputes immediately.
  const [requiredConfigured, setRequiredConfigured] = useState<{ llm: boolean; asr: boolean }>({
    llm: false,
    asr: false,
  });
  useEffect(() => {
    let cancelled = false;
    const load = () => {
      void Promise.all([listChannels('llm'), listChannels('asr')])
        .then(([llm, asr]) => {
          if (cancelled) return;
          setRequiredConfigured({
            llm: llm.some((channel) => channel.enabled),
            asr: asr.some((channel) => channel.enabled),
          });
        })
        .catch(() => {
          /* On read failure, keep the previous state and don't disturb the user */
        });
    };
    load();
    window.addEventListener('ol-channels-changed', load);
    return () => {
      cancelled = true;
      window.removeEventListener('ol-channels-changed', load);
    };
  }, []);

  return (
    <>
      <div className="ol-services-page">
        <div className="ol-service-pipeline-mode">
          <SettingRow
            className="ol-service-pipeline-row"
            label={t('settings.providers.pipelineModeLabel')}
            desc={t('settings.providers.pipelineModeHint')}
          >
            <div
              ref={pipelineRef}
              className="ol-service-pipeline-track"
              role="group"
              aria-label={t('settings.providers.pipelineModeLabel')}
              style={segmentedTrackStyle}
            >
              <span
                ref={pipelineIndicatorRef}
                aria-hidden
                className="ol-service-pipeline-indicator"
              />
              {(['traditional', 'multimodal'] as const).map((mode) => (
                <button
                  key={mode}
                  type="button"
                  disabled={!prefs}
                  aria-pressed={(multimodal ? 'multimodal' : 'traditional') === mode}
                  onClick={() => setPipelineMode(mode)}
                  className="ol-service-pipeline-option"
                >
                  {t(
                    `settings.providers.pipelineMode${mode === 'traditional' ? 'Traditional' : 'Multimodal'}`,
                  )}
                </button>
              ))}
            </div>
          </SettingRow>
          <p className="ol-service-pipeline-hint">
            {t('settings.providers.pipelineIsolationNotice')}
          </p>
        </div>
        <div
          ref={navigationRef}
          role="group"
          aria-label={t('modal.serviceViews.label')}
          className="ol-service-views"
          style={{ gridTemplateColumns: `repeat(${views.length}, minmax(0, 1fr))` }}
        >
          <span ref={navigationIndicatorRef} aria-hidden className="ol-service-view-indicator" />
          {views.map((id) => {
            const inactive = isServiceViewInactive(id, multimodal);
            const required = !inactive && (id === 'llm' || id === 'asr');
            const configured =
              id === 'llm' ? requiredConfigured.llm : id === 'asr' ? requiredConfigured.asr : false;
            const label = t(`modal.serviceViews.${id}`);
            const inactiveLabel = t('modal.serviceViews.inactive');
            return (
              <button
                key={id}
                type="button"
                disabled={inactive}
                aria-pressed={selectedView === id}
                onClick={() => setView(id)}
                aria-label={
                  inactive
                    ? `${label}。${inactiveLabel}`
                    : required
                      ? `${label}。${t(
                          configured
                            ? 'modal.serviceViews.statusConfigured'
                            : 'modal.serviceViews.statusMissing',
                        )}`
                      : label
                }
                title={
                  inactive
                    ? inactiveLabel
                    : required
                      ? t(
                          configured
                            ? 'modal.serviceViews.statusConfigured'
                            : 'modal.serviceViews.statusMissing',
                        )
                      : undefined
                }
              >
                <span className="ol-service-view-label">
                  {required && (
                    <span
                      aria-hidden
                      className="ol-service-status-dot"
                      data-state={configured ? 'ok' : 'missing'}
                    />
                  )}
                  <span>{label}</span>
                </span>
                {inactive && <span className="ol-service-inactive-tag">{inactiveLabel}</span>}
              </button>
            );
          })}
        </div>
        <div key={selectedView} ref={contentRef} className="ol-service-content">
          {/* The LocalModelPicker inside the channel editor jumps to this view through this context. */}
          <LocalModelsNavContext.Provider
            value={showLocalModel && !multimodal ? () => setView('models') : null}
          >
            {selectedView === 'llm' && <ProvidersSection kind="llm" />}
            {selectedView === 'asr' && <ProvidersSection kind="asr" />}
            {selectedView === 'omni' && <OmniChannelSection />}
            {selectedView === 'models' && <LocalModelsSection />}
            {selectedView === 'connections' && (
              <>
                <NetworkSection />
                <MarketplaceSection />
              </>
            )}
          </LocalModelsNavContext.Provider>
          {(selectedView === 'llm' || selectedView === 'asr') && (
            <p className="ol-service-storage-note">
              {t('settings.providers.credentialStorageNotice')}
            </p>
          )}
        </div>
      </div>
    </>
  );
}

// Privacy: local-first explanation + permission management · data storage.
export function PrivacyTab() {
  const { t } = useTranslation();
  return (
    <>
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 10,
          padding: '10px 12px',
          borderRadius: 10,
          background: 'var(--ol-blue-soft)',
          marginBottom: 2,
        }}
      >
        <span
          style={{
            fontSize: 11,
            padding: '3px 8px',
            borderRadius: 999,
            background: 'var(--ol-surface)',
            color: 'var(--ol-blue)',
            fontWeight: 600,
            flexShrink: 0,
          }}
        >
          {t('modal.about.localFirst')}
        </span>
        <span style={{ fontSize: 11.5, color: 'var(--ol-ink-3)', lineHeight: 1.55 }}>
          {t('modal.about.privacyDesc')}
        </span>
      </div>
      <PermissionsSection />
      <DataStorageSection />
      <CloudSyncSection />
    </>
  );
}

// Experimental features each get a subpage in the right column; navigating back and forth preserves console drafts, test state, and streaming output.
export function AdvancedTab({
  pages,
  page,
  onOpenPage,
}: {
  pages: AdvancedPage[];
  page: AdvancedPageId | null;
  onOpenPage: (page: AdvancedPageId) => void;
}) {
  const { t } = useTranslation();
  return (
    <>
      <div hidden={page !== null}>
        <div className="ol-advanced-settings-list">
          {pages.map((item) => (
            <button
              key={item.id}
              type="button"
              className="ol-advanced-settings-entry"
              data-ol-advanced-entry={item.id}
              onClick={() => onOpenPage(item.id)}
            >
              <span className="ol-advanced-entry-icon">
                <Icon name={item.icon} size={19} />
              </span>
              <span className="ol-advanced-entry-copy">
                <span className="ol-advanced-entry-title">{t(item.titleKey)}</span>
                <span className="ol-advanced-entry-description">
                  {t(`modal.advancedPages.${item.id}`)}
                </span>
              </span>
              <Icon name="chevRight" size={17} className="ol-advanced-entry-arrow" />
            </button>
          ))}
        </div>
      </div>
      {pages.map((item) => (
        <section
          key={item.id}
          hidden={page !== item.id}
          className="ol-advanced-settings-detail"
          aria-label={t(item.titleKey)}
          data-ol-advanced-page={item.id}
        >
          {item.id === 'lessComputer' && <CodingAgentSection />}
          {item.id === 'claudeConsole' && <ClaudeConsoleSection />}
          {item.id === 'vocabularyLearning' && <VocabularyLearningSection />}
          {item.id === 'debug' && <DebugToolsSection />}
        </section>
      ))}
    </>
  );
}

// About: version info · update channel · auto-update — "which version am I on and how do I update" in one place.
export function AboutTab() {
  const platformCaps = usePlatformCaps();
  const showUpdateControls = platformCaps?.supportsAutoUpdate === true;

  return (
    <>
      <AboutSection />
      {showUpdateControls && <BetaChannelSection />}
      {showUpdateControls && <AutoUpdateSection />}
    </>
  );
}
