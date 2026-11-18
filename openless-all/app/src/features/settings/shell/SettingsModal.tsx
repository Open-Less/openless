// Settings categories reuse the existing consumers; complex experiments open in the right pane.
import { useRef, useState, type CSSProperties } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../../../components/Icon';
import { openExternal } from '../../../api/files';
import { useMobileLayout, useConservativeLayout } from '../../../lib/useMobileLayout';
import type { OS } from '../../../runtime/environment';
import {
  AboutTab,
  GeneralTab,
  InputMethodTab,
  ServicesTab,
  CloudServicesTab,
  PrivacyTab,
  AdvancedTab,
  ShortcutsTab,
  AppearanceTab,
  VoiceEnhancementDepthContext,
} from './tabs';
import type { SettingsSectionId, ServiceViewId } from './navigation';
import { useSettingsNavigation } from './useSettingsNavigation';
import { useSettingsLayer } from './useSettingsLayer';
import { PopupContainer, ViewportPortal } from '../../../ui/layout/AppViewport';
import './settings-workspace.css';
import { ChannelEditorHostContext } from './ChannelEditorHostContext';
import { useHotkeySettings } from '../../../data/preferences';
import {
  useExperimentalAsr,
  ExperimentalAsrGuidance,
} from '../../local-asr/ExperimentalAsrSection';
import { ProviderLeaveContext, useProviderForm } from '../../providers/ProviderForm';
import { TransitionViewport } from '../../../ui/layout/TransitionViewport';

export type { SettingsSectionId } from './navigation';

interface SettingsModalProps {
  os: OS;
  onClose: () => void;
  initialSettingsSection?: SettingsSectionId;
  initialServiceView?: ServiceViewId;
  /** Keeps the closing surface mounted until its exit finishes. */
  closing?: boolean;
  onPaintedChange?: (ready: boolean) => void;
}

const LINKS = [
  { id: 'helpCenter', icon: 'help', href: 'https://github.com/Open-Less/openless#readme' },
  { id: 'releaseNotes', icon: 'doc', href: 'https://github.com/Open-Less/openless/releases' },
];

export function SettingsModal({
  os,
  onClose,
  initialSettingsSection,
  initialServiceView,
  closing = false,
  onPaintedChange,
}: SettingsModalProps) {
  const { t } = useTranslation();
  // The provider form's writes settle before close/section switch (async saves don't
  // lose drafts); the revision counter excludes credential content.
  const providerForm = useProviderForm();
  const mobile = useMobileLayout();
  const conservative = useConservativeLayout();
  const channelCloseRef = useRef<(() => void) | null>(null);
  const [channelContainer, setChannelContainer] = useState<HTMLDivElement | null>(null);
  const [channelBackground, setChannelBackground] = useState<HTMLDivElement | null>(null);
  const { updatePrefs } = useHotkeySettings();
  const nav = useSettingsNavigation({
    os,
    initialSettingsSection,
    initialServiceView,
    channelCloseRef,
    beforeLeave: providerForm.finish,
    onClose,
  });
  const {
    advancedPage,
    advancedPages,
    activeAdvancedPage,
    activeGeneralPage,
    onGeneralPageChange,
    focusReturnedEntry,
    asrNavigation,
    backToAdvanced,
    closeSettings,
    closeRef,
    contentDescription,
    contentTitle,
    destination,
    jumpToAsr,
    openAdvancedPage,
    openLocalModels,
    platformCaps,
    query,
    requestedServiceView,
    results,
    searchRef,
    searching,
    section,
    sections,
    selectSection,
    setQuery,
    showingVoiceSettings,
    scrollRef,
    surfaceRef,
    voiceSettingsOpen,
    setVoiceSettingsOpen,
  } = nav;
  const experimentalAsr = useExperimentalAsr(platformCaps?.platform === 'desktop' && os === 'mac');
  const openExperimentalAsrChannels = async () => {
    const navigated = await providerForm.finish(async () => {
      await updatePrefs((current) => ({ ...current, pipelineMode: 'traditional' }));
      channelCloseRef.current?.();
      jumpToAsr();
    });
    if (!navigated) throw new Error(t('common.operationFailed'));
  };
  const overlayRef = useRef<HTMLDivElement>(null);
  const layer = useSettingsLayer({
    overlayRef,
    surfaceRef,
    closing,
    mobile,
    initialFocus: mobile ? closeRef : searchRef,
    onEscape: nav.onEscape,
    onPaintedChange,
  });

  // Search box: desktop puts it at the top of the sidebar (mirroring macOS System
  // Settings' "search above the nav" layout); mobile keeps it full-width under the
  // title bar.
  const searchBox = (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 8,
        padding: '7px 10px',
        borderRadius: 'var(--ol-control-radius)',
        border: '0.5px solid var(--ol-line-strong)',
        background: 'var(--ol-surface)',
        minWidth: 0,
      }}
    >
      <Icon name="search" size={15} />
      <input
        ref={searchRef}
        type="search"
        value={query}
        onChange={(event) => {
          nav.changeQuery(event.target.value);
        }}
        placeholder={t('modal.searchPlaceholder')}
        aria-label={t('modal.searchPlaceholder')}
        style={{
          width: '100%',
          minWidth: 0,
          border: 0,
          outline: 'none',
          background: 'transparent',
          color: 'var(--ol-ink)',
          font: 'inherit',
          fontSize: 14,
        }}
      />
      {query && (
        <button
          type="button"
          aria-label={t('modal.clearSearch')}
          onClick={() => {
            setQuery('');
            searchRef.current?.focus();
          }}
          style={{ ...iconButtonStyle, width: 22, height: 22 }}
        >
          <Icon name="close" size={12} />
        </button>
      )}
    </div>
  );

  return (
    <ProviderLeaveContext.Provider value={providerForm.register}>
      <ViewportPortal>
        <div
          ref={overlayRef}
          className="ol-settings-workspace"
          style={{
            position: layer.boundary ? 'absolute' : mobile ? 'fixed' : 'absolute',
            inset: 0,
            background: 'var(--ol-surface)',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            padding: 0,
            zIndex: layer.priority,
            pointerEvents: closing ? 'none' : undefined,
          }}
        >
          <PopupContainer.Provider value={layer.popupHost}>
            <div
              ref={surfaceRef}
              role="dialog"
              aria-label={t('shell.footer.settings')}
              className="ol-settings-surface"
              data-ol-mobile={mobile ? 'true' : undefined}
              onClick={(event) => event.stopPropagation()}
              tabIndex={-1}
              style={{
                width: '100%',
                maxWidth: '100%',
                height: '100%',
                maxHeight: '100%',
                minHeight: 0,
                background: 'var(--ol-settings-content-bg)',
                borderRadius: 0,
                border: 'none',
                boxShadow: 'none',
                display: 'flex',
                flexDirection: 'column',
                overflow: 'hidden',
              }}
            >
              {/* Desktop no longer has a title bar spanning both columns;
            the sidebar and content each run to the top, with title/auto-save/close
            folded into the right column's top. Mobile keeps the full-width header
            (title + close + full-row search). */}
              {mobile && (
                <header
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    flexWrap: 'wrap',
                    gap: 12,
                    padding: 'calc(12px + env(safe-area-inset-top, 0px)) 16px 12px',
                    borderBottom: '0.5px solid var(--ol-line-soft)',
                    flexShrink: 0,
                  }}
                >
                  <div
                    style={{
                      flex: 1,
                      order: 0,
                      fontSize: 17,
                      fontWeight: 'var(--ol-font-weight-demibold)',
                    }}
                  >
                    {t('shell.footer.settings')}
                  </div>
                  <button
                    ref={closeRef}
                    type="button"
                    onClick={closeSettings}
                    aria-label={t('common.close')}
                    style={{ ...iconButtonStyle, order: 1 }}
                  >
                    <Icon name="close" size={17} />
                  </button>
                  <div style={{ order: 2, flex: '1 0 100%', minWidth: 0 }}>{searchBox}</div>
                </header>
              )}
              <div
                style={{
                  flex: 1,
                  minHeight: 0,
                  display: 'flex',
                  flexDirection: mobile ? 'column' : 'row',
                }}
              >
                <aside
                  className="ol-settings-rail"
                  style={{
                    flexShrink: 0,
                    minHeight: 0,
                    overflow: 'auto',
                    padding: mobile ? '8px 12px' : os === 'mac' ? '48px 12px 16px' : '20px 12px',
                    background: 'var(--ol-settings-rail-bg)',
                    borderRight: mobile ? undefined : '0.5px solid var(--ol-line-soft)',
                    borderBottom: mobile ? '0.5px solid var(--ol-line-soft)' : undefined,
                    display: 'flex',
                    flexDirection: 'column',
                    gap: 12,
                  }}
                >
                  {!mobile && (
                    <button
                      ref={closeRef}
                      type="button"
                      className="ol-settings-return ol-settings-rail-link"
                      onClick={closeSettings}
                    >
                      <Icon name="chevLeft" size={16} />
                      {t('uiRefinement.back')}
                    </button>
                  )}
                  {!mobile && searchBox}
                  <nav
                    aria-label={t('modal.categoriesLabel')}
                    className="ol-thinscroll"
                    style={{
                      display: 'flex',
                      flexDirection: mobile ? 'row' : 'column',
                      gap: 4,
                      overflowX: mobile ? 'auto' : undefined,
                    }}
                  >
                    {/* States (idle / hover / selected) come from .ol-settings-rail-btn, the
                    same selection language as the main sidebar. */}
                    {sections.map((item) => {
                      const active = !searching && section === item.id;
                      return (
                        <button
                          key={item.id}
                          type="button"
                          aria-current={active ? 'page' : undefined}
                          onClick={() => selectSection(item.id)}
                          className="ol-settings-rail-btn"
                          style={{
                            ...railButtonStyle,
                            padding: mobile ? '8px 10px' : '10px',
                          }}
                        >
                          {!mobile && <Icon name={item.icon} size={16} />}
                          <span className="ol-settings-rail-label">{item.title}</span>
                        </button>
                      );
                    })}
                  </nav>
                  {!mobile && <HelpLinks />}
                </aside>
                <div
                  ref={setChannelContainer}
                  className="ol-settings-content-pane"
                  data-ol-advanced={section === 'advanced' && !searching ? 'true' : undefined}
                  style={{
                    flex: 1,
                    minWidth: 0,
                    minHeight: 0,
                    display: 'flex',
                    flexDirection: 'column',
                    position: 'relative',
                  }}
                >
                  <ChannelEditorHostContext.Provider
                    value={{
                      container: channelContainer,
                      background: channelBackground,
                      registerClose: (close) => {
                        channelCloseRef.current = close;
                      },
                    }}
                  >
                    <div
                      ref={setChannelBackground}
                      style={{ display: 'flex', flexDirection: 'column', flex: 1, minHeight: 0 }}
                    >
                      <TransitionViewport
                        mode="page"
                        header={
                          <>
                            {/* The toolbar belongs to its page; retained forms keep their matching title. */}
                            {(!mobile || activeAdvancedPage || activeGeneralPage) && (
                              <div
                                className="ol-settings-page-toolbar"
                                style={{
                                  padding: mobile
                                    ? '18px var(--ol-grid-margin, 24px) 0'
                                    : os === 'mac'
                                      ? '48px var(--ol-grid-margin, 28px) 0'
                                      : '24px var(--ol-grid-margin, 28px) 0',
                                }}
                              >
                                <div className="ol-settings-page-title-group">
                                  {(activeAdvancedPage || activeGeneralPage) && (
                                    <button
                                      type="button"
                                      className="ol-settings-back"
                                      onClick={
                                        activeAdvancedPage
                                          ? backToAdvanced
                                          : () => onGeneralPageChange('root')
                                      }
                                      aria-label={t('modal.back')}
                                      title={t('modal.back')}
                                    >
                                      <Icon name="chevLeft" size={19} />
                                    </button>
                                  )}
                                  <h2 className="ol-settings-page-title" tabIndex={-1}>
                                    {contentTitle}
                                    {activeAdvancedPage &&
                                      'beta' in activeAdvancedPage &&
                                      activeAdvancedPage.beta && (
                                        <span className="ol-beta-tag">Beta</span>
                                      )}
                                  </h2>
                                </div>
                                {!mobile && (
                                  <span className="ol-settings-auto-save">{t('modal.autoSaveHint')}</span>
                                )}
                              </div>
                            )}
                            <div
                              style={{
                                padding: mobile ? '18px 16px 12px' : '8px 28px 12px',
                                flexShrink: 0,
                              }}
                            >
                              {mobile && !activeAdvancedPage && !activeGeneralPage && (
                                <h2
                                  className="ol-settings-page-title ol-settings-page-title--compact"
                                  tabIndex={-1}
                                >
                                  {contentTitle}
                                </h2>
                              )}
                              {activeAdvancedPage?.id !== 'localModels' && (
                                <p
                                  style={{
                                    margin:
                                      mobile && !activeAdvancedPage && !activeGeneralPage
                                        ? '7px 0 0'
                                        : 0,
                                    fontSize: 13,
                                    lineHeight: 1.6,
                                    color: 'var(--ol-ink-3)',
                                  }}
                                  aria-live="polite"
                                >
                                  {contentDescription}
                                </p>
                              )}
                            </div>
                          </>
                        }
                        scrollRef={scrollRef}
                        onEntered={focusReturnedEntry}
                        contentKey={`${destination}:${showingVoiceSettings ? 'dsp' : 'page'}:${searching ? 'search' : 'content'}`}
                        className={[
                          'ol-thinscroll ol-settings-scroll',
                          conservative ? 'ol-conservative-scope' : '',
                        ].join(' ')}
                        style={{
                          flex: 1,
                          minHeight: 0,
                          overflow: 'auto',
                          padding: mobile
                            ? '0 16px calc(20px + env(safe-area-inset-bottom, 0px))'
                            : section === 'services'
                              ? '0 28px 6px'
                              : '0 28px 28px',
                        }}
                      >
                        <VoiceEnhancementDepthContext.Provider
                          value={{ open: voiceSettingsOpen, setOpen: setVoiceSettingsOpen }}
                        >
                          {searching && (
                            <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
                              {results.map((item) => (
                                <button
                                  key={item.id}
                                  type="button"
                                  onClick={() => selectSection(item.id, true)}
                                  style={{
                                    ...navButtonStyle,
                                    border: '0.5px solid var(--ol-line)',
                                    padding: 16,
                                    alignItems: 'flex-start',
                                    background: 'var(--ol-surface)',
                                  }}
                                >
                                  <Icon name={item.icon} size={18} />
                                  <span style={{ flex: 1, minWidth: 0 }}>
                                    <span
                                      style={{
                                        display: 'block',
                                        fontWeight: 'var(--ol-font-weight-semibold)',
                                        marginBottom: 5,
                                      }}
                                    >
                                      {item.title}
                                    </span>
                                    <span
                                      style={{
                                        fontSize: 12,
                                        color: 'var(--ol-ink-3)',
                                        lineHeight: 1.6,
                                      }}
                                    >
                                      {item.description}
                                    </span>
                                  </span>
                                  <Icon name="chevRight" size={14} />
                                </button>
                              ))}
                              {results.length === 0 && (
                                <div
                                  style={{
                                    padding: '24px 16px',
                                    border: '0.5px solid var(--ol-line)',
                                    borderRadius: 'var(--ol-r-md)',
                                    textAlign: 'center',
                                  }}
                                >
                                  <p style={{ fontSize: 13, color: 'var(--ol-ink-3)' }}>
                                    {t('modal.noResults')}
                                  </p>
                                  <button
                                    type="button"
                                    onClick={() => {
                                      setQuery('');
                                      searchRef.current?.focus();
                                    }}
                                    style={{
                                      ...navButtonStyle,
                                      margin: '12px auto 0',
                                      color: 'var(--ol-blue)',
                                    }}
                                  >
                                    {t('modal.clearSearch')}
                                  </button>
                                </div>
                              )}
                            </div>
                          )}
                          {/* Category replacement keeps each form's lifecycle separate. */}
                          <div
                            key={section}
                            className="ol-settings-sections"
                            data-section={section}
                            style={{
                              display: searching ? 'none' : 'flex',
                              flexDirection: 'column',
                              gap: 16,
                            }}
                          >
                            {section === 'general' && (
                              <GeneralTab
                                os={os}
                                platform={platformCaps?.platform}
                                onOpenLocalModels={openLocalModels}
                                page={activeGeneralPage?.id ?? 'root'}
                                onPageChange={onGeneralPageChange}
                              />
                            )}
                            {section === 'inputMethod' && <InputMethodTab />}
                            {section === 'shortcuts' && <ShortcutsTab />}
                            {section === 'appearance' && <AppearanceTab />}
                            {section === 'services' && (
                              <ServicesTab
                                asrNavigation={asrNavigation}
                                initialView={requestedServiceView}
                                beforeLeave={providerForm.finish}
                              />
                            )}
                            {section === 'cloudServices' && <CloudServicesTab />}
                            {section === 'privacy' && <PrivacyTab />}
                            {section === 'advanced' && (
                              <AdvancedTab
                                pages={advancedPages}
                                page={advancedPage}
                                onOpenPage={openAdvancedPage}
                                experimentalAsr={experimentalAsr}
                                onOpenAsr={openExperimentalAsrChannels}
                              />
                            )}
                            {section === 'about' && <AboutTab />}
                          </div>
                          {mobile && !searching && <HelpLinks />}
                        </VoiceEnhancementDepthContext.Provider>
                      </TransitionViewport>
                    </div>
                  </ChannelEditorHostContext.Provider>
                </div>
              </div>
            </div>
          </PopupContainer.Provider>
          <div ref={layer.setPopupHost} />
        </div>
      </ViewportPortal>
      <ExperimentalAsrGuidance
        controller={experimentalAsr}
        onOpenAsr={openExperimentalAsrChannels}
      />
    </ProviderLeaveContext.Provider>
  );
}

function HelpLinks() {
  const { t } = useTranslation();
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        gap: 4,
        paddingTop: 16,
        marginTop: 20,
        borderTop: '0.5px solid var(--ol-line-soft)',
      }}
    >
      {LINKS.map((link) => (
        <button
          key={link.id}
          type="button"
          onClick={() => void openExternal(link.href)}
          className="ol-settings-rail-link"
          style={{ ...railButtonStyle, padding: '8px 10px' }}
        >
          <Icon name={link.icon} size={14} />
          <span style={{ flex: 1 }}>{t(`modal.sections.${link.id}`)}</span>
          <Icon name="external" size={11} />
        </button>
      ))}
    </div>
  );
}

const iconButtonStyle: CSSProperties = {
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  flexShrink: 0,
  width: 30,
  height: 30,
  padding: 0,
  border: 0,
  borderRadius: 'var(--ol-control-radius)',
  background: 'transparent',
  color: 'var(--ol-ink-3)',
  cursor: 'pointer',
};
const navButtonStyle: CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  gap: 10,
  padding: '8px 10px',
  border: 0,
  borderRadius: 'var(--ol-control-radius)',
  background: 'transparent',
  color: 'var(--ol-ink-2)',
  font: 'inherit',
  fontSize: 14,
  cursor: 'pointer',
  textAlign: 'left',
};
/** Rail rows: layout only. Colour, weight and background belong to the state classes;
 * a `font` shorthand here would pin the weight and hide the selected state. */
const railButtonStyle: CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  gap: 10,
  flexShrink: 0,
  border: 0,
  borderRadius: 'var(--ol-control-radius)',
  fontFamily: 'inherit',
  fontSize: 14,
  textAlign: 'left',
};
