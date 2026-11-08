// Content page for each sidebar tab in the settings dialog. Each tab is a vertical stack of section cards;
// the real logic lives in the *Section files — this only decides which sections belong to which tab.

import { useTranslation } from 'react-i18next';
import { createContext, useContext, useEffect, useRef, useState } from 'react';
import { AndroidPermissionsPanel } from '@android/components/AndroidPermissionsPanel';
import { Icon } from '../../components/Icon';
import { Tooltip } from '../../components/Tooltip';
import { detectOS } from '../../components/WindowChrome';
import { ExperimentalAsrSection, type ExperimentalAsrController } from './ExperimentalAsrSection';
import { RecordingInputSection } from './RecordingInputSection';
import { RemoteInputSection } from './RemoteInputSection';
import { ShortcutsSection } from './ShortcutsSection';
import { SelectionWorkspaceSection } from './SelectionWorkspaceSection';
import { LanguageSection } from './LanguageSection';
import { TypographySection } from './TypographySection';
import { ThemeSection } from './ThemeSection';
import { ProvidersSection } from './ChannelList';
import { OmniChannelSection } from './ProvidersSection';
import { NetworkSection } from './NetworkSection';
import { MarketplaceSection } from './MarketplaceSection';
import { PermissionsSection } from './PermissionsSection';
import { DataStorageSection } from './DataStorageSection';
import { CloudSyncSection, CustomSyncServerSection } from './CloudSyncSection';
import { LocalMigrationSection } from './LocalMigrationSection';
import { LocalModelsSection } from './models/LocalModelsSection';
import { LocalModelsNavContext } from './models/modelsNav';
import { DebugToolsSection } from './DebugToolsSection';
import { VocabularyLearningSection } from './VocabularyLearningSection';
import { ExpressionLearningSection } from './ExpressionLearningSection';
import { AudioDspSettings } from './AudioDspSettings';
import { CodingAgentSection } from './CodingAgentSection';
import { ClaudeConsoleSection } from './ClaudeConsoleSection';
import { BetaChannelSection } from './BetaChannelSection';
import { AutoUpdateSection } from './AutoUpdateSection';
import { StartupDiagnostics } from './StartupDiagnostics';
import { AboutSection } from './AboutSection';
import { getPlatformCapabilities, getCachedPlatformCapabilities } from '../../lib/platform';
import { listChannels } from '../../lib/ipc';
import type { PlatformCapabilities } from '../../lib/types';
import { type OS } from '../../components/WindowChrome';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { emitSaved } from '../../lib/savedEvent';
import { SettingRow, Toggle, ExperimentalSectionTitle, inputStyle } from './shared';
import { Segmented } from '../../components/ui/Segmented';
import { Card } from '../_atoms';
import { useContentMotion, useSelectionMotion } from '../../lib/motion';
import { TransitionViewport } from '../../components/ui/TransitionViewport';
import {
  advancedPageGroup,
  isInlineAdvancedPage,
  availableServiceViews,
  isTestingFeaturePage,
  isServiceViewInactive,
  resolveServiceView,
  showsCursorContextControl,
  type ServiceViewId,
  type AdvancedPage,
  type AdvancedPageId,
} from './navigation';

// Platform capability query shared by all tabs (drives desktop/mobile and hotkey / auto-update gating).
function usePlatformCaps(): PlatformCapabilities | null {
  const [platformCaps, setPlatformCaps] = useState(getCachedPlatformCapabilities);

  useEffect(() => {
    void getPlatformCapabilities().then(setPlatformCaps);
  }, []);

  return platformCaps;
}

// General: microphone, capsule style, text insertion, and remote input.
// Each category is one page. Pages do not collapse their own groups again.
export function GeneralTab({
  os,
  onOpenLocalModels,
}: { os?: OS; onOpenLocalModels?: () => void } = {}) {
  const platformCaps = usePlatformCaps();
  const showRemoteInput = platformCaps?.platform === 'desktop';
  const showCapsule = platformCaps == null || platformCaps.platform === 'desktop';
  const [page, setPage] = useState<'root' | 'recording' | 'capsule' | 'textInput' | 'phone'>(
    'root',
  );
  const activePage =
    (page === 'phone' && !showRemoteInput) || (page === 'capsule' && !showCapsule) ? 'root' : page;
  const contentRef = useRef<HTMLDivElement>(null);
  useContentMotion(contentRef, activePage);
  const categories = [
    {
      id: 'recording' as const,
      icon: 'mic',
      title: 'settings.generalPages.recordingTitle',
      description: 'settings.generalPages.recordingDesc',
    },
    {
      id: 'capsule' as const,
      icon: 'layout',
      title: 'settings.generalPages.capsuleTitle',
      description: 'settings.generalPages.capsuleDesc',
    },
    {
      id: 'textInput' as const,
      icon: 'pencil',
      title: 'settings.generalPages.textInputTitle',
      description: 'settings.generalPages.textInputDesc',
    },
    {
      id: 'phone' as const,
      icon: 'link',
      title: 'settings.generalPages.phoneTitle',
      description: 'settings.generalPages.phoneDesc',
    },
  ].filter(
    (item) => (item.id !== 'phone' || showRemoteInput) && (item.id !== 'capsule' || showCapsule),
  );
  const pageTitle = {
    recording: 'settings.generalPages.recordingTitle',
    capsule: 'settings.generalPages.capsuleTitle',
    textInput: 'settings.generalPages.textInputTitle',
    phone: 'settings.generalPages.phoneTitle',
  } as const;
  if (activePage !== 'root') {
    return (
      <div ref={contentRef} style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
        <SettingsSubpageHeader onBack={() => setPage('root')} title={pageTitle[activePage]} />
        {activePage === 'recording' && (
          <RecordingInputSection
            forcedOs={os}
            onOpenLocalModels={onOpenLocalModels}
            surface="microphone"
          />
        )}
        {activePage === 'capsule' && <RecordingInputSection forcedOs={os} surface="capsule" />}
        {activePage === 'textInput' && (
          <>
            <SelectionWorkspaceSection showShortcut={false} />
            <RecordingInputSection forcedOs={os} surface="text" />
          </>
        )}
        {activePage === 'phone' && <RemoteInputSection />}
      </div>
    );
  }
  return (
    <div ref={contentRef} className="ol-settings-category-list">
      {categories.map((item) => (
        <SettingsCategoryEntry
          key={item.id}
          icon={item.icon}
          title={item.title}
          description={item.description}
          onClick={() => setPage(item.id)}
        />
      ))}
    </div>
  );
}

function SettingsSubpageHeader({ title, onBack }: { title: string; onBack?: () => void }) {
  const { t } = useTranslation();
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
      {onBack && (
        <button type="button" className="ol-settings-back-button" onClick={onBack}>
          <Icon name="chevLeft" size={16} />
          {t('modal.back')}
        </button>
      )}
      <h2 style={{ margin: 0, fontSize: 16, fontWeight: 'var(--ol-font-weight-semibold)' }}>
        {t(title)}
      </h2>
    </div>
  );
}

function SettingsCategoryEntry({
  icon,
  title,
  description,
  onClick,
}: {
  icon: string;
  title: string;
  description: string;
  onClick: () => void;
}) {
  const { t } = useTranslation();
  return (
    <button type="button" className="ol-settings-category-entry" onClick={onClick}>
      <span className="ol-settings-category-icon">
        <Icon name={icon} size={18} />
      </span>
      <span className="ol-settings-category-copy">
        <strong>{t(title)}</strong>
        <span>{t(description)}</span>
      </span>
      <Icon name="chevRight" size={17} />
    </button>
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
  return <ShortcutsSection />;
}

export function AppearanceTab() {
  return (
    <>
      <ThemeSection />
      <TypographySection />
      <LanguageSection />
    </>
  );
}

// AI services & models: organizes channels, local models, and network settings by current pipeline and platform capabilities.
export function ServicesTab({
  asrNavigation = 0,
  initialView,
}: { asrNavigation?: number; initialView?: ServiceViewId } = {}) {
  const { t } = useTranslation();
  const { prefs, updatePrefs } = useHotkeySettings();
  const platformCaps = usePlatformCaps();
  const showLocalModel = detectOS() !== 'mac' && platformCaps?.supportsLocalAsr === true;
  const multimodal =
    prefs?.multimodalPipelineEnabled === true && prefs.pipelineMode === 'multimodal';
  const [view, setView] = useState<ServiceViewId>(initialView ?? (asrNavigation ? 'asr' : 'llm'));
  useEffect(() => {
    if (initialView) setView(initialView);
    else if (asrNavigation) setView('asr');
  }, [initialView, asrNavigation]);
  const voiceActivationModels =
    showLocalModel && detectOS() === 'win' && prefs?.voiceActivationEnabled === true;
  const views = availableServiceViews(showLocalModel, detectOS());
  const requestedView = resolveServiceView(
    view,
    views,
    multimodal,
    detectOS(),
    voiceActivationModels,
  );
  const contentRef = useRef<HTMLDivElement>(null);
  const selectedView = requestedView;
  const navigationRef = useRef<HTMLDivElement>(null);
  const navigationIndicatorRef = useRef<HTMLSpanElement>(null);
  useSelectionMotion(
    navigationRef,
    navigationIndicatorRef,
    `${requestedView}:${views.length}`,
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
            <Segmented
              className="ol-service-pipeline-track"
              indicatorClassName="ol-service-pipeline-indicator"
              label={t('settings.providers.pipelineModeLabel')}
              disabled={!prefs}
              value={multimodal ? 'multimodal' : 'traditional'}
              onChange={setPipelineMode}
              options={(['traditional', 'multimodal'] as const).map((mode) => ({
                value: mode,
                label: t(
                  `settings.providers.pipelineMode${mode === 'traditional' ? 'Traditional' : 'Multimodal'}`,
                ),
              }))}
            />
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
            if (id === 'models' && detectOS() === 'mac') {
              return (
                <Tooltip
                  key={id}
                  content={t('experimentalAsr.retiredHint')}
                  placement="bottom"
                  wrap
                  focusable
                  trigger={<span />}
                >
                  <button
                    type="button"
                    disabled
                    data-retired-local-models
                    aria-label={t('modal.serviceViews.models')}
                    style={{ pointerEvents: 'none' }}
                  >
                    <span>{t('modal.serviceViews.models')}</span>
                    <span className="ol-service-inactive-tag">
                      {t('experimentalAsr.retiredLabel')}
                    </span>
                  </button>
                </Tooltip>
              );
            }
            const inactive = isServiceViewInactive(
              id,
              multimodal,
              detectOS(),
              voiceActivationModels,
            );
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
                aria-pressed={requestedView === id}
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
        <TransitionViewport
          contentKey={selectedView}
          scrollRef={contentRef}
          className="ol-service-content ol-thinscroll"
          style={{ overflow: 'auto' }}
        >
          {/* The LocalModelPicker inside the channel editor jumps to this view through this context. */}
          <LocalModelsNavContext.Provider
            value={
              showLocalModel && (!multimodal || voiceActivationModels)
                ? () => setView('models')
                : null
            }
          >
            {selectedView === 'llm' && <ProvidersSection kind="llm" />}
            {selectedView === 'asr' && <ProvidersSection kind="asr" />}
            {selectedView === 'omni' && <OmniChannelSection />}
            {showLocalModel && selectedView === 'models' && (
              <>
                {multimodal && voiceActivationModels && (
                  <p className="ol-service-pipeline-hint">
                    {t('settings.recording.voiceActivationMultimodalModels')}
                  </p>
                )}
                <LocalModelsSection />
              </>
            )}
            {selectedView === 'connections' && <NetworkSection />}
          </LocalModelsNavContext.Provider>
          {(selectedView === 'llm' || selectedView === 'asr') && (
            <p className="ol-service-storage-note">
              {t('settings.providers.credentialStorageNotice')}
            </p>
          )}
        </TransitionViewport>
      </div>
    </>
  );
}

export function CloudServicesTab() {
  return (
    <>
      <MarketplaceSection />
      <CloudSyncSection />
    </>
  );
}

export const VoiceEnhancementDepthContext = createContext<{
  open: boolean;
  setOpen: (open: boolean) => void;
} | null>(null);

function VoiceEnhancementSection() {
  const { t } = useTranslation();
  const { prefs, updatePrefs } = useHotkeySettings();
  const platformCaps = usePlatformCaps();
  const depth = useContext(VoiceEnhancementDepthContext);
  if (!prefs) return null;
  const dspOpen = depth?.open ?? false;
  const openDsp = () => depth?.setOpen(true);
  if (dspOpen) {
    return (
      <div>
        <AudioDspSettings
          page
          value={prefs.audioDsp}
          onChange={(audioDsp) => void updatePrefs((current) => ({ ...current, audioDsp }))}
        />
      </div>
    );
  }
  return (
    <div>
      <Card>
        <ExperimentalSectionTitle badge={t('common.experimental')}>
          {t('settings.advancedPages.voiceEnhancementTitle')}
        </ExperimentalSectionTitle>
        {platformCaps?.platform === 'desktop' && (
          <SettingRow
            label={t('settings.recording.echoCancellationLabel')}
            desc={t('settings.recording.echoCancellationDesc')}
          >
            <Toggle
              on={prefs.echoCancellationEnabled ?? false}
              onToggle={(next) =>
                void updatePrefs((current) => ({ ...current, echoCancellationEnabled: next }))
              }
            />
          </SettingRow>
        )}
        <SettingRow
          label={t('settings.recording.audioEnhancementLabel')}
          desc={t('settings.recording.audioEnhancementDesc')}
        >
          <Toggle
            on={prefs.audioEnhancementEnabled}
            onToggle={(next) =>
              void updatePrefs((current) => ({ ...current, audioEnhancementEnabled: next }))
            }
          />
        </SettingRow>
      </Card>
      <div className="ol-advanced-settings-list" style={{ marginTop: 12 }}>
        <button type="button" className="ol-advanced-settings-entry" onClick={openDsp}>
          <span className="ol-advanced-entry-icon">
            <Icon name="mic" size={19} />
          </span>
          <span className="ol-advanced-entry-copy">
            <span className="ol-advanced-entry-title">{t('settings.recording.dsp.title')}</span>
            <span className="ol-advanced-entry-description">
              {t('settings.recording.dsp.description')}
            </span>
          </span>
          <Icon name="chevRight" size={17} className="ol-advanced-entry-arrow" />
        </button>
      </div>
    </div>
  );
}

function CursorContextSection() {
  const { t } = useTranslation();
  const { prefs, updatePrefs } = useHotkeySettings();
  const platformCaps = usePlatformCaps();
  if (!prefs) return null;
  const showCursor = showsCursorContextControl(platformCaps?.platform, detectOS());
  // Empty input rolls back to off. 50 characters keeps the skip on short fragments.
  const onPolishSkipMaxCharsChange = (raw: string) => {
    const parsed = raw === '' ? 0 : Number.parseInt(raw, 10);
    if (Number.isNaN(parsed)) return;
    void updatePrefs((current) => ({
      ...current,
      polishSkipMaxChars: Math.max(0, Math.min(50, parsed)),
    }));
  };
  return (
    <Card>
      <ExperimentalSectionTitle badge={t('common.experimental')}>
        {t('settings.advancedPages.cursorContextTitle')}
      </ExperimentalSectionTitle>
      {showCursor && (
        <SettingRow
          label={t('settings.dataStorage.cursorContextLabel')}
          desc={t('settings.dataStorage.cursorContextDesc')}
        >
          <Toggle
            on={prefs.cursorContextEnabled}
            onToggle={(next) =>
              void updatePrefs((current) => ({ ...current, cursorContextEnabled: next }))
            }
          />
        </SettingRow>
      )}
      {showCursor && (
        <p className="ol-section-note">{t('settings.advancedPages.cursorContextHint')}</p>
      )}
      <SettingRow
        label={t('settings.recording.polishSkipMaxCharsLabel')}
        desc={t('settings.recording.polishSkipMaxCharsDesc')}
      >
        <input
          type="number"
          min={0}
          max={50}
          value={prefs.polishSkipMaxChars}
          onChange={(event) => onPolishSkipMaxCharsChange(event.target.value)}
          style={{ ...inputStyle, width: 80, textAlign: 'right' }}
        />
      </SettingRow>
    </Card>
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
            fontWeight: 'var(--ol-font-weight-semibold)',
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
      <LocalMigrationSection />
    </>
  );
}

// Experimental features each get a subpage in the right column; navigating back and forth preserves console drafts, test state, and streaming output.
export function AdvancedTab({
  pages,
  page,
  onOpenPage,
  experimentalAsr,
  onOpenAsr,
}: {
  pages: AdvancedPage[];
  page: AdvancedPageId | null;
  onOpenPage: (page: AdvancedPageId) => void;
  experimentalAsr: ExperimentalAsrController;
  onOpenAsr: () => Promise<void>;
}) {
  const { t } = useTranslation();
  const platformCaps = usePlatformCaps();
  const [visited, setVisited] = useState<AdvancedPageId[]>([]);
  useEffect(() => {
    if (page) setVisited((current) => (current.includes(page) ? current : [...current, page]));
  }, [page]);
  const { prefs, updatePrefs } = useHotkeySettings();
  const testing = prefs?.testingFeaturesEnabled === true;
  const [savingTesting, setSavingTesting] = useState(false);
  const testingReady = testing && !savingTesting;
  const entries = pages.filter((item) =>
    page === 'testingFeatures'
      ? testingReady && isTestingFeaturePage(item.id)
      : !isTestingFeaturePage(item.id),
  );
  const list = (
    <div className="ol-advanced-settings-list">
      {entries.map((item, index) => {
        const group = advancedPageGroup(item);
        const previous = index > 0 ? advancedPageGroup(entries[index - 1]) : undefined;
        const showGroup = group !== undefined && group !== previous;
        return (
          <div key={item.id}>
            {showGroup && (
              <div className="ol-advanced-settings-group-title">
                {t(`settings.advancedGroups.${group}`)}
              </div>
            )}
            {isInlineAdvancedPage(item) ? (
              <div className="ol-advanced-settings-inline">
                {item.id === 'voiceEnhancement' && <VoiceEnhancementSection />}
                {item.id === 'startupDiagnostics' && <StartupDiagnostics />}
              </div>
            ) : (
              <button
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
                {item.id === 'testingFeatures' && (
                  <span style={{ color: 'var(--ol-blue)', fontSize: 11, fontWeight: 600 }}>
                    Beta
                  </span>
                )}
                <Icon name="chevRight" size={17} className="ol-advanced-entry-arrow" />
              </button>
            )}
          </div>
        );
      })}
    </div>
  );
  return (
    <>
      {page === null && list}
      {page === 'testingFeatures' && (
        <Card>
          <ExperimentalSectionTitle badge="Beta">
            {t('settings.testingFeatures.title')}
          </ExperimentalSectionTitle>
          <SettingRow
            label={t('settings.testingFeatures.enable')}
            desc={t('settings.testingFeatures.description')}
          >
            <Toggle
              on={testing}
              disabled={!prefs || savingTesting}
              label={t('settings.testingFeatures.enable')}
              onToggle={async (next) => {
                setSavingTesting(true);
                try {
                  await updatePrefs((current) => ({ ...current, testingFeaturesEnabled: next }));
                } catch {
                  emitSaved('failed', t('common.operationFailed'));
                } finally {
                  setSavingTesting(false);
                }
              }}
            />
          </SettingRow>
          {testingReady && list}
          {testingReady && entries.length === 0 && (
            <p style={{ color: 'var(--ol-ink-3)', fontSize: 12 }}>
              {t('settings.testingFeatures.unavailable')}
            </p>
          )}
        </Card>
      )}
      {pages
        .filter(
          (item) =>
            item.id !== 'testingFeatures' &&
            !isInlineAdvancedPage(item) &&
            visited.includes(item.id),
        )
        .map((item) => (
          <section
            key={item.id}
            hidden={page !== item.id}
            className="ol-advanced-settings-detail"
            aria-label={t(item.titleKey)}
            data-ol-advanced-page={item.id}
          >
            {item.id === 'localModels' && (
              <ExperimentalAsrSection controller={experimentalAsr} onOpenAsr={onOpenAsr} />
            )}
            {testingReady && item.id === 'lessComputer' && <CodingAgentSection />}
            {testingReady && item.id === 'claudeConsole' && <ClaudeConsoleSection />}
            {item.id === 'voiceEnhancement' && <VoiceEnhancementSection />}
            {item.id === 'cursorContext' && (
              <>
                <CursorContextSection />
                {showsCursorContextControl(platformCaps?.platform, detectOS()) && (
                  <VocabularyLearningSection />
                )}
                {platformCaps?.platform === 'desktop' &&
                  (detectOS() === 'mac' || detectOS() === 'win') && <ExpressionLearningSection />}
              </>
            )}
            {item.id === 'startupDiagnostics' && <StartupDiagnostics />}
            {item.id === 'customSyncServer' && page === item.id && <CustomSyncServerSection />}
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
