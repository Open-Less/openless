// Content page for each sidebar tab in the settings dialog. Each tab is a vertical stack of section cards;
// the real logic lives in the *Section files — this only decides which sections belong to which tab.

import { useTranslation } from 'react-i18next';
import { createContext, useContext, useEffect, useRef, useState } from 'react';
import { AndroidPermissionsPanel } from '@android/components/AndroidPermissionsPanel';
import { Icon } from '../../../components/Icon';
import { isRetiredMacAsrProvider } from '../../../lib/experimentalAsr';
import { getRuntimeOS } from '../../../runtime/environment';
import {
  ExperimentalAsrSection,
  type ExperimentalAsrController,
} from '../../local-asr/ExperimentalAsrSection';
import { RecordingInputSection } from '../../input/RecordingInputSection';
import { RemoteInputSection } from '../../remote-input/RemoteInputSection';
import { ShortcutsSection } from '../../input/ShortcutsSection';
import { SelectionWorkspaceSection } from '../../input/SelectionWorkspaceSection';
import { LanguageSection } from '../sections/LanguageSection';
import { TypographySection } from '../sections/TypographySection';
import { ThemeSection } from '../sections/ThemeSection';
import { ProvidersSection } from '../../providers/ChannelList';
import { OmniChannelSection } from '../../providers/ProvidersSection';
import { WebSearchSection } from '../sections/WebSearchSection';
import { ensureDeepSeekSuiteConfig } from '../../providers/deepseekSuite';
import { DeepSeekSuiteSection } from '../../providers/DeepSeekSuiteSection';
import { NetworkSection } from '../sections/NetworkSection';
import { MarketplaceSection } from '../sections/MarketplaceSection';
import { PermissionsSection } from '../../input/PermissionsSection';
import { DataStorageSection } from '../sections/DataStorageSection';
import { CloudSyncSection, CustomSyncServerSection } from '../../cloud-sync/CloudSyncSection';
import { LocalMigrationSection } from '../sections/LocalMigrationSection';
import { LocalModelsSection } from '../../local-asr/settings/LocalModelsSection';
import { LocalModelsNavContext } from '../../local-asr/settings/modelsNav';
import { DebugToolsSection } from '../sections/DebugToolsSection';
import { VocabularyLearningSection } from '../../input/VocabularyLearningSection';
import { ExpressionLearningSection } from '../../input/ExpressionLearningSection';
import { AudioDspSettings } from '../../input/AudioDspSettings';
import { BetaChannelSection } from '../sections/BetaChannelSection';
import { AutoUpdateSection } from '../sections/AutoUpdateSection';
import { StartupDiagnostics } from '../sections/StartupDiagnostics';
import { AboutSection } from '../sections/AboutSection';
import { getPlatformCapabilities, getCachedPlatformCapabilities } from '../../../runtime/platform';
import { listChannels, listenCredentialsChanges } from '../../../api/providers';
import type { PipelineMode, PlatformCapabilities, PlatformKind } from '../../../api/contracts';
import { type OS } from '../../../runtime/environment';
import { useHotkeySettings } from '../../../data/preferences';
import { emitSaved } from '../../../lib/savedEvent';
import { SettingRow, Toggle } from '../../../ui/controls/SettingRow';
import { ExperimentalSectionTitle } from '../../../ui/layout/ExperimentalSectionTitle';
import { inputStyle } from '../../../ui/controls/settingsFieldStyles';
import { Segmented } from '../../../components/ui/Segmented';
import { Card } from '../../../ui/layout/Card';
import { TransitionViewport } from '../../../components/ui/TransitionViewport';
import {
  availableServiceViews,
  isServiceViewInactive,
  resolveServiceView,
  showsCursorContextControl,
  visibleGeneralPages,
  type GeneralPageId,
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
// Each category is one page. The settings shell owns the page id so its top bar can
// render the subpage title and the shared back control.
export function GeneralTab({
  os,
  platform,
  onOpenLocalModels,
  page,
  onPageChange,
}: {
  os?: OS;
  platform?: PlatformKind;
  onOpenLocalModels?: () => void;
  page: GeneralPageId | 'root';
  onPageChange: (page: GeneralPageId | 'root') => void;
}) {
  const categories = visibleGeneralPages(platform);
  const activePage = page;
  if (activePage !== 'root') {
    return (
      <div style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
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
    <div className="ol-settings-category-list">
      {categories.map((item) => (
        <SettingsCategoryEntry
          key={item.id}
          id={item.id}
          general
          icon={item.icon}
          title={item.titleKey}
          description={item.descriptionKey}
          onClick={() => onPageChange(item.id)}
        />
      ))}
    </div>
  );
}

function SettingsCategoryEntry({
  id,
  general = false,
  icon,
  title,
  description,
  beta = false,
  onClick,
}: {
  id?: string;
  general?: boolean;
  icon: string;
  title: string;
  description: string;
  beta?: boolean;
  onClick: () => void;
}) {
  const { t } = useTranslation();
  return (
    <button
      type="button"
      className="ol-settings-category-entry"
      data-ol-general-entry={general ? id : undefined}
      data-ol-advanced-entry={general ? undefined : id}
      onClick={onClick}
    >
      <span className="ol-settings-category-icon">
        <Icon name={icon} size={18} />
      </span>
      <span className="ol-settings-category-copy">
        <strong>
          <span>{t(title)}</span>
          {beta && <span className="ol-beta-tag">Beta</span>}
        </strong>
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

function UnavailableServiceView({
  label,
  hint,
  retired = false,
}: {
  label: string;
  hint: string;
  retired?: boolean;
}) {
  return (
    <span className="ol-service-view-slot">
      <button
        type="button"
        disabled
        data-retired-local-models={retired || undefined}
        aria-label={`${label}。${hint}`}
      >
        <span className="ol-service-view-label">
          <span>{label}</span>
        </span>
      </button>
      <span className="ol-service-unavailable" aria-hidden>
        {hint}
      </span>
    </span>
  );
}

// AI services & models: organizes channels, local models, and network settings by current pipeline and platform capabilities.
export function ServicesTab({
  asrNavigation = 0,
  initialView,
  beforeLeave,
}: {
  asrNavigation?: number;
  initialView?: ServiceViewId;
  beforeLeave?: (action: () => void | Promise<void>) => Promise<boolean>;
} = {}) {
  const { t } = useTranslation();
  const { prefs, updatePrefs } = useHotkeySettings();
  const platformCaps = usePlatformCaps();
  const os = getRuntimeOS();
  const showLocalModel = os !== 'mac' && platformCaps?.supportsLocalAsr === true;
  const multimodal =
    prefs?.multimodalPipelineEnabled === true && prefs.pipelineMode === 'multimodal';
  const [view, setView] = useState<ServiceViewId>(initialView ?? (asrNavigation ? 'asr' : 'llm'));
  useEffect(() => {
    if (initialView) setView(initialView);
    else if (asrNavigation) setView('asr');
  }, [initialView, asrNavigation]);
  const voiceActivationModels =
    showLocalModel && os === 'win' && prefs?.voiceActivationEnabled === true;
  const views = availableServiceViews(showLocalModel, os);
  const requestedView = resolveServiceView(view, views, multimodal, os, voiceActivationModels);
  const contentRef = useRef<HTMLDivElement>(null);
  const suiteMode = prefs?.pipelineMode === 'deepseek';
  const selectedView = suiteMode ? 'deepseek' : requestedView;
  const pipelineMode: PipelineMode = suiteMode
    ? 'deepseek'
    : multimodal
      ? 'multimodal'
      : 'traditional';
  const pipelineSwitch = useRef(false);
  const [switching, setSwitching] = useState(false);
  const changeService = (action: () => void | Promise<void>) =>
    beforeLeave
      ? beforeLeave(action)
      : Promise.resolve()
          .then(action)
          .then(() => true);
  const selectView = (id: ServiceViewId) => {
    void changeService(() => setView(id));
  };
  const setPipelineMode = (mode: PipelineMode) => {
    if (!prefs || pipelineMode === mode || pipelineSwitch.current) return;
    pipelineSwitch.current = true;
    setSwitching(true);
    // Flush the old form before its pipeline disappears. Commit the preference
    // after the new pipeline has read or initialized its independent credentials.
    void changeService(async () => {
      if (mode === 'deepseek') await ensureDeepSeekSuiteConfig();
      await updatePrefs((current) => ({
        ...current,
        pipelineMode: mode,
        multimodalPipelineEnabled: true,
      }));
    })
      .catch(() => emitSaved('failed', t('common.operationFailed')))
      .finally(() => {
        pipelineSwitch.current = false;
        setSwitching(false);
      });
  };
  const [requiredConfigured, setRequiredConfigured] = useState({ llm: false, asr: false });
  useEffect(() => {
    if (suiteMode) return;
    let cancelled = false;
    let revision = 0;
    let unlisten: (() => void) | undefined;
    const load = () => {
      const observed = ++revision;
      void Promise.all([listChannels('llm'), listChannels('asr')])
        .then(([llm, asr]) => {
          if (cancelled || observed !== revision) return;
          setRequiredConfigured({
            llm: llm.some((channel) => channel.enabled),
            asr: asr.some(
              (channel) =>
                channel.enabled && !(os === 'mac' && isRetiredMacAsrProvider(channel.providerType)),
            ),
          });
        })
        .catch(() => undefined);
    };
    void listenCredentialsChanges(load).then(
      (stop) => {
        if (cancelled) stop();
        else {
          unlisten = stop;
          load();
        }
      },
      () => {
        if (!cancelled) load();
      },
    );
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [suiteMode, os]);

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
              disabled={!prefs || switching}
              value={pipelineMode}
              onChange={setPipelineMode}
              options={(['traditional', 'multimodal', 'deepseek'] as const).map((mode) => ({
                value: mode,
                label: t(
                  `settings.providers.pipelineMode${mode === 'traditional' ? 'Traditional' : mode === 'multimodal' ? 'Multimodal' : 'Deepseek'}`,
                ),
              }))}
            />
          </SettingRow>
          <p className="ol-service-pipeline-hint">
            {t('settings.providers.pipelineIsolationNotice')}
          </p>
        </div>
        {!suiteMode && (
          <div
            role="group"
            aria-label={t('modal.serviceViews.label')}
            className="ol-service-views"
            style={{ gridTemplateColumns: `repeat(${views.length}, minmax(0, 1fr))` }}
          >
            {views.map((id) => {
              if (id === 'models' && os === 'mac') {
                return (
                  <UnavailableServiceView
                    key={id}
                    label={t('modal.serviceViews.models')}
                    hint={t('modal.serviceViews.unavailable')}
                    retired
                  />
                );
              }
              const inactive = isServiceViewInactive(id, multimodal, os, voiceActivationModels);
              const required = !inactive && (id === 'llm' || id === 'asr');
              const configured =
                id === 'llm'
                  ? requiredConfigured.llm
                  : id === 'asr'
                    ? requiredConfigured.asr
                    : false;
              const label = t(`modal.serviceViews.${id}`);
              if (inactive) {
                return (
                  <UnavailableServiceView
                    key={id}
                    label={label}
                    hint={t('modal.serviceViews.unavailable')}
                  />
                );
              }
              return (
                <button
                  key={id}
                  type="button"
                  aria-pressed={requestedView === id}
                  onClick={() => selectView(id)}
                  aria-label={
                    required && !configured
                      ? `${label}。${t('modal.serviceViews.statusMissing')}`
                      : label
                  }
                  title={
                    required && !configured ? t('modal.serviceViews.statusMissing') : undefined
                  }
                >
                  <span className="ol-service-view-label">
                    <span>{label}</span>
                  </span>
                  {required && !configured && (
                    <span aria-hidden className="ol-service-status-dot" data-state="missing" />
                  )}
                </button>
              );
            })}
          </div>
        )}
        <TransitionViewport
          mode="page"
          contentKey={selectedView}
          scrollRef={contentRef}
          className="ol-service-content ol-thinscroll"
          style={{ overflow: 'auto' }}
        >
          {/* The LocalModelPicker inside the channel editor jumps to this view through this context. */}
          <LocalModelsNavContext.Provider
            value={
              showLocalModel && (!multimodal || voiceActivationModels)
                ? () => selectView('models')
                : null
            }
          >
            {selectedView === 'llm' && <ProvidersSection kind="llm" />}
            {selectedView === 'asr' && <ProvidersSection kind="asr" />}
            {selectedView === 'omni' && <OmniChannelSection />}
            {selectedView === 'deepseek' && <DeepSeekSuiteSection />}
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
  const showCursor = showsCursorContextControl(platformCaps?.platform, getRuntimeOS());
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

function LiveInputSection() {
  const { t } = useTranslation();
  const { prefs, updatePrefs } = useHotkeySettings();
  if (!prefs || getRuntimeOS() !== 'win') return null;
  return (
    <Card>
      <ExperimentalSectionTitle badge="Beta">
        {t('settings.advancedPages.liveInputTitle')}
      </ExperimentalSectionTitle>
      <SettingRow
        label={t('settings.advanced.windowsCompositionInputLabel')}
        desc={t('settings.advanced.windowsCompositionInputDesc')}
      >
        <Toggle
          on={prefs.windowsCompositionInput}
          disabled={prefs.windowsInsertionMode !== 'tsf'}
          onToggle={(next) =>
            void updatePrefs((current) => ({ ...current, windowsCompositionInput: next }))
          }
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
          borderRadius: 'var(--ol-r-md)',
          background: 'var(--ol-blue-soft)',
          marginBottom: 2,
        }}
      >
        <span
          style={{
            fontSize: 11,
            padding: '3px 8px',
            borderRadius: 'var(--ol-radius)',
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
  const entries = pages;
  const list = (
    <div className="ol-advanced-settings-list">
      {entries.map((item) => (
        <SettingsCategoryEntry
          id={item.id}
          key={item.id}
          icon={item.icon}
          title={item.titleKey}
          beta={'beta' in item && item.beta}
          description={`modal.advancedPages.${item.id}`}
          onClick={() => onOpenPage(item.id)}
        />
      ))}
    </div>
  );
  return (
    <>
      {page === null && list}
      {pages
        .filter((item) => item.id === page)
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
            {item.id === 'voiceEnhancement' && <VoiceEnhancementSection />}
            {item.id === 'liveInput' && <LiveInputSection />}
            {item.id === 'cursorContext' && (
              <div
                className="ol-awareness-sections"
                style={{ display: 'flex', flexDirection: 'column', gap: 16 }}
              >
                <CursorContextSection />
                {showsCursorContextControl(platformCaps?.platform, getRuntimeOS()) && (
                  <VocabularyLearningSection />
                )}
                {platformCaps?.platform === 'desktop' &&
                  (getRuntimeOS() === 'mac' || getRuntimeOS() === 'win') && (
                    <ExpressionLearningSection />
                  )}
              </div>
            )}
            {item.id === 'webSearch' && <WebSearchSection />}
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
