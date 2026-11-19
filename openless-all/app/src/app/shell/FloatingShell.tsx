// Main window shell: sidebar navigation, page switching, mobile panels, and the
// settings modal. Pages share preference state; business reads/writes go to Core via typed IPC.

import { Suspense, useEffect, useRef, useState, type CSSProperties } from 'react';
import { afterPaint } from '../../lib/startupMetrics';
import { guideFinishedThisSession } from '../../lib/desktopOnboarding';
import { useTranslation } from 'react-i18next';
import { Icon } from '../../components/Icon';
import { WindowChrome } from './WindowChrome';
import { DesktopSidebar } from './DesktopSidebar';
import './shell-navigation.css';
import { getRuntimeOS, getShortcutPlatform, type OS } from '../../runtime/environment';
import { SettingsModal, type SettingsSectionId } from '../../features/settings/shell/SettingsModal';
import type { ServiceViewId } from '../../features/settings/shell/navigation';
import { CloudSyncSetupPrompt } from '../../features/cloud-sync/CloudSyncSetupPrompt';
import {
  HOTKEY_MODE_MIGRATION_ACK_KEY,
  HOTKEY_MODE_MIGRATION_DEFERRED_KEY,
  shouldShowHotkeyModeMigrationPrompt,
} from '../../lib/hotkeyMigration';
import { useExitMount } from '../../lib/useExitMount';
import { useOverlayMotion } from '../../lib/motion';
import { TransitionViewport } from '../../ui/layout/TransitionViewport';
import { getCredentials } from '../../api/providers';
import {
  PROVIDER_SETUP_PROMPT_DEFERRED_KEY,
  shouldShowProviderSetupPrompt,
} from '../../lib/providerSetup';
import { MobileMoreSheet } from './MobileMoreSheet';
import { MobileStyleSheet } from './MobileStyleSheet';
import { applyStackedLayoutFromPrefs } from '../../lib/stackedLayout';
import { applyConservativeLayout } from '../../lib/conservativeLayout';
import { useMobileLayout, useConservativeLayout } from '../../lib/useMobileLayout';
import { useHotkeySettings } from '../../data/preferences';
import {
  MOBILE_BOTTOM_TABS,
  MORE_TABS,
  STYLE_TABS,
  subItemLabelKey,
  useNavigation,
  type AppTab,
} from './navigation';
import { getPage, preloadPage, Overview, SelectionAsk } from './pages';

interface FloatingShellProps {
  os?: OS;
  initialTab?: AppTab;
  initialSettings?: boolean;
}

export function FloatingShell({
  os: osProp,
  initialTab = 'overview',
  initialSettings = false,
}: FloatingShellProps) {
  const os = osProp ?? getRuntimeOS();
  const mobile = useMobileLayout();
  return (
    <WindowChrome os={os} mobile={mobile} title="OpenLess" height="100%">
      <FloatingShellBody
        os={os}
        mobile={mobile}
        initialTab={initialTab}
        initialSettings={initialSettings}
      />
    </WindowChrome>
  );
}

function FloatingShellBody({
  os,
  mobile,
  initialTab,
  initialSettings,
}: {
  os: OS;
  mobile: boolean;
  initialTab: AppTab;
  initialSettings: boolean;
}) {
  const { t } = useTranslation();
  const { isMac } = getShortcutPlatform();
  const conservative = useConservativeLayout();
  const { prefs } = useHotkeySettings();
  const { currentTab, setCurrentTab, settingsOpen, setSettingsOpen, openGroups, toggleGroup } =
    useNavigation(initialTab, initialSettings);
  const [settingsInitialSection, setSettingsInitialSection] = useState<
    SettingsSectionId | undefined
  >();
  const [settingsInitialServiceView, setSettingsInitialServiceView] = useState<ServiceViewId>();
  const [providerPromptOpen, setProviderPromptOpen] = useState(false);
  const [hotkeyModePromptOpen, setHotkeyModePromptOpen] = useState(false);
  const settingsMount = useExitMount(settingsOpen);
  const providerPromptMount = useExitMount(providerPromptOpen);
  const hotkeyPromptMount = useExitMount(hotkeyModePromptOpen);
  const [moreOpen, setMoreOpen] = useState(false);
  const [styleOpen, setStyleOpen] = useState(false);
  const pageRef = useRef<HTMLDivElement>(null);
  const displayTab = currentTab;

  // Already applied before React mounts; no first-frame scale jump.
  useEffect(() => {
    if (prefs) return afterPaint('interactive');
  }, [!!prefs]);

  useEffect(() => {
    applyStackedLayoutFromPrefs(prefs?.stackedRowLayout);
    applyConservativeLayout(prefs?.conservativeLayout === true);
  }, [prefs?.stackedRowLayout, prefs?.conservativeLayout]);

  const Page = getPage(displayTab);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const credentials = await getCredentials();
        const promptDeferredValue = window.sessionStorage.getItem(
          PROVIDER_SETUP_PROMPT_DEFERRED_KEY,
        );
        if (!cancelled && shouldShowProviderSetupPrompt(credentials, promptDeferredValue)) {
          setProviderPromptOpen(true);
        }
      } catch (error) {
        // A locked/unavailable credential store is not an unconfigured provider.
        console.warn('[startup] credential status unavailable', error);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    const acknowledgedValue = window.localStorage.getItem(HOTKEY_MODE_MIGRATION_ACK_KEY);
    const deferredValue = window.sessionStorage.getItem(HOTKEY_MODE_MIGRATION_DEFERRED_KEY);
    if (shouldShowHotkeyModeMigrationPrompt(acknowledgedValue, deferredValue)) {
      setHotkeyModePromptOpen(true);
    }
  }, []);

  // The old NAVIGATE_LOCAL_ASR_EVENT listener is obsolete: the standalone "model
  // settings" tab is gone — model management now renders via <LocalAsr embedded /> in
  // Settings → Services, all in one place.

  const rememberProviderPrompt = () => {
    window.sessionStorage.setItem(PROVIDER_SETUP_PROMPT_DEFERRED_KEY, '1');
    setProviderPromptOpen(false);
  };

  const deferHotkeyModePrompt = () => {
    window.sessionStorage.setItem(HOTKEY_MODE_MIGRATION_DEFERRED_KEY, '1');
    setHotkeyModePromptOpen(false);
  };

  const openSettings = (section?: SettingsSectionId, serviceView?: ServiceViewId) => {
    setSettingsInitialSection(section);
    setSettingsInitialServiceView(serviceView);
    setSettingsOpen(true);
    setMoreOpen(false);
    setStyleOpen(false);
  };

  // Platform-appropriate settings shortcut.
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if ((isMac ? e.metaKey : e.ctrlKey) && e.key === ',') {
        e.preventDefault();
        openSettings();
      }
    };
    window.addEventListener('keydown', onKeyDown, true);
    return () => window.removeEventListener('keydown', onKeyDown, true);
  }, [isMac]);

  const openProviderSettings = () => {
    rememberProviderPrompt();
    openSettings('services');
  };

  const openHotkeyRecordingSettings = () => {
    window.localStorage.setItem(HOTKEY_MODE_MIGRATION_ACK_KEY, '1');
    setHotkeyModePromptOpen(false);
    openSettings('general');
  };

  const mobileTitle = settingsOpen ? t('shell.footer.settings') : t(subItemLabelKey(currentTab));
  const moreTabActive = MORE_TABS.some((tab) => tab.id === currentTab);
  const styleTabActive = STYLE_TABS.some((tab) => tab.id === currentTab);

  return (
    // Window content flush to the top; macOS reserves the traffic-light strip inside
    // the sidebar only.
    <div
      style={{
        flex: 1,
        position: 'relative',
        display: 'flex',
        flexDirection: 'column',
        minHeight: 0,
        paddingTop: 0,
        background: 'var(--ol-app-shell-bg)',
      }}
    >
      {mobile && (
        <div style={{ display: 'contents' }}>
          <MobileTopBar
            title={mobileTitle}
            onOpenSettings={() => openSettings()}
            settingsActive={settingsOpen}
          />
        </div>
      )}

      {/* Main shell — flush with the frosted backplate (no separate float). */}
      <div
        data-ol-settings-open={settingsOpen ? 'true' : undefined}
        className="ol-app-shell-bg ol-shell-depth"
        style={{
          flex: 1,
          minHeight: 0,
          display: 'flex',
          overflow: 'hidden',
          position: 'relative',
          zIndex: 1,
        }}
      >
        {!mobile && (
          <DesktopSidebar
            os={os}
            currentTab={currentTab}
            settingsOpen={settingsOpen}
            openGroups={openGroups}
            onSelectTab={setCurrentTab}
            onPreloadTab={preloadPage}
            onToggleGroup={toggleGroup}
            onOpenSettings={openSettings}
          />
        )}

        {/* Main content — one flat solid block (feedback: the right side is a single
            slab, no rounding, not a floating card). Solid surface, no margin, no
            rounding, no shadow, flush to the window edge, separated from the sidebar
            only by the sidebar's border-right. mac rounding comes from the native
            window clip. */}
        <div style={{ flex: 1, minWidth: 0, padding: 0, display: 'flex' }}>
          <main
            className="ol-console-main"
            style={{
              flex: 1,
              minWidth: 0,
              overflow: 'hidden',
              background: 'var(--ol-surface)',
              borderRadius: 0,
              border: 'none',
              boxShadow: 'none',
              display: 'flex',
              flexDirection: 'column',
            }}
          >
            {/* Padding and overflow live on the transitioning page wrapper:
                  - naturally-sized pages (Overview / Vocab / Style): the wrapper scrolls
                    when the page content overflows
                  - height:100% pages (History's two columns): 100% resolves against the
                    wrapper's fixed height so each column's own overflow:auto can scroll */}
            <TransitionViewport
              mode="page"
              scrollRef={pageRef}
              contentKey={displayTab}
              page={displayTab}
              // Other tabs scroll the page. Desktop overview is one dashboard: the
              // page surface does not scroll, and only the recent-recognition list does.
              // Mobile overview stays a growing column because the same blocks stack.
              className={
                displayTab === 'overview' && !mobile
                  ? 'ol-thinscroll ol-page-fixed'
                  : displayTab === 'overview'
                    ? 'ol-thinscroll'
                    : 'ol-thinscroll ol-scroll-fade'
              }
              style={{
                flex: 1,
                minHeight: 0,
                overflow: displayTab === 'overview' && !mobile ? 'hidden' : 'auto',
                boxSizing: 'border-box',
                // macOS: 26px title vs 15px wordmark, so the title starts 6px above the
                // traffic-light clearance and lines up with "OpenLess". Other desktops
                // keep a positive inset and never subtract the native caption height,
                // so a Windows overlay title bar cannot cover the title.
                padding: mobile
                  ? '16px var(--ol-grid-margin-compact) calc(16px + env(safe-area-inset-bottom, 0px) + 56px)'
                  : `${
                      os === 'mac'
                        ? 'calc(var(--ol-shell-mac-top-inset) - 6px)'
                        : 'max(8px, calc(var(--ol-window-titlebar-height, 0px) / var(--ol-interface-scale, 1)))'
                    } var(--ol-grid-margin) ${displayTab === 'overview' ? 'var(--ol-grid-margin)' : '32px'}`,
                // position:relative lets the page's "saved" toast anchor at absolute
                // top:16 right:16 to this console card's corner instead of stretching
                // across the page header as a full-width banner.
                position: 'relative',
                display: 'flex',
                flexDirection: 'column',
              }}
            >
              {displayTab === 'overview' ? (
                <Overview
                  onOpenHistory={() => setCurrentTab('history')}
                  onOpenSettings={openSettings}
                />
              ) : (
                <div
                  className={conservative ? 'ol-conservative-scope' : undefined}
                  style={{
                    display: 'flex',
                    flexDirection: 'column',
                    // Keep the usage-guide content height so the outer scroll area's
                    // bottom-nav spacing applies.
                    flex: displayTab === 'selectionAsk' ? '1 0 auto' : 1,
                    minHeight: 0,
                  }}
                >
                  {displayTab === 'selectionAsk' ? (
                    <SelectionAsk onOpenShortcuts={() => openSettings('shortcuts')} />
                  ) : (
                    <Page />
                  )}
                </div>
              )}
            </TransitionViewport>
          </main>
        </div>
      </div>

      {mobile && (
        <>
          {!settingsOpen && (
            <MobileBottomNav
              currentTab={currentTab}
              moreOpen={moreOpen}
              moreTabActive={moreTabActive}
              styleOpen={styleOpen}
              styleTabActive={styleTabActive}
              settingsOpen={settingsOpen}
              onSelectTab={(id) => {
                setMoreOpen(false);
                setStyleOpen(false);
                setCurrentTab(id);
              }}
              onOpenStyle={() => {
                setMoreOpen(false);
                setStyleOpen(true);
              }}
              onOpenMore={() => {
                setStyleOpen(false);
                setMoreOpen(true);
              }}
            />
          )}
          <MobileStyleSheet
            open={styleOpen}
            currentTab={currentTab}
            onClose={() => setStyleOpen(false)}
            onSelectTab={(id) => {
              setStyleOpen(false);
              setCurrentTab(id);
            }}
          />
          <MobileMoreSheet
            open={moreOpen}
            currentTab={currentTab}
            onClose={() => setMoreOpen(false)}
            onSelectTab={setCurrentTab}
            onOpenSettings={() => openSettings()}
          />
        </>
      )}

      {/* Settings modal — rendered inside this window; settingsMount gates the exit animation */}
      {settingsMount.mounted && (
        <Suspense
          // SettingsModal is eagerly loaded so the first open cannot flash a
          // different placeholder surface while the settings chunk resolves.
          fallback={<div className="ol-settings-placeholder" aria-hidden="true" />}
        >
          <SettingsModal
            key={`${settingsInitialSection ?? 'default'}:${settingsInitialServiceView ?? 'default'}`}
            os={os}
            closing={settingsMount.closing}
            initialSettingsSection={settingsInitialSection}
            initialServiceView={settingsInitialServiceView}
            onClose={() => setSettingsOpen(false)}
          />
        </Suspense>
      )}

      {providerPromptMount.mounted ? (
        <ProviderSetupPrompt
          closing={providerPromptMount.closing}
          onLater={rememberProviderPrompt}
          onOpenSettings={openProviderSettings}
          onRestore={() => {
            rememberProviderPrompt();
            openSettings('cloudServices');
          }}
        />
      ) : hotkeyPromptMount.mounted ? (
        <HotkeyModeMigrationPrompt
          closing={hotkeyPromptMount.closing}
          onLater={deferHotkeyModePrompt}
          onOpenSettings={openHotkeyRecordingSettings}
        />
      ) : null}
      <CloudSyncSetupPrompt
        blocked={
          guideFinishedThisSession() ||
          settingsMount.mounted ||
          providerPromptMount.mounted ||
          hotkeyPromptMount.mounted
        }
        onSetup={() => openSettings('cloudServices')}
      />
    </div>
  );
}

function MobileTopBar({
  title,
  onOpenSettings,
  settingsActive,
}: {
  title: string;
  onOpenSettings: () => void;
  settingsActive: boolean;
}) {
  const { t } = useTranslation();
  return (
    <header
      style={{
        flexShrink: 0,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        gap: 12,
        padding: 'calc(10px + env(safe-area-inset-top, 0px)) 14px 10px',
        borderBottom: '0.5px solid var(--ol-line-soft)',
        background: 'var(--ol-surface)',
        zIndex: 2,
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, minWidth: 0 }}>
        <img
          src="AppIcon.png"
          alt=""
          style={{ width: 22, height: 22, borderRadius: 5, flexShrink: 0 }}
        />
        <span
          style={{
            fontSize: 16,
            fontWeight: 'var(--ol-font-weight-semibold)',
            letterSpacing: '-0.02em',
            color: 'var(--ol-ink)',
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
          }}
        >
          {title}
        </span>
      </div>
      <button
        type="button"
        onClick={onOpenSettings}
        aria-label={t('shell.footer.settings')}
        className={settingsActive ? 'ol-nav-btn ol-nav-btn-active' : 'ol-nav-btn'}
        style={{
          width: 36,
          height: 36,
          flexShrink: 0,
          border: 0,
          borderRadius: 10,
          background: settingsActive ? 'var(--ol-surface-2)' : 'transparent',
          display: 'inline-flex',
          alignItems: 'center',
          justifyContent: 'center',
          cursor: 'default',
        }}
      >
        <Icon name="settings" size={18} />
      </button>
    </header>
  );
}

function MobileBottomNav({
  currentTab,
  moreOpen,
  moreTabActive,
  styleOpen,
  styleTabActive,
  settingsOpen,
  onSelectTab,
  onOpenStyle,
  onOpenMore,
}: {
  currentTab: AppTab;
  moreOpen: boolean;
  moreTabActive: boolean;
  styleOpen: boolean;
  styleTabActive: boolean;
  settingsOpen: boolean;
  onSelectTab: (tab: AppTab) => void;
  onOpenStyle: () => void;
  onOpenMore: () => void;
}) {
  const { t } = useTranslation();
  const moreActive = moreOpen || moreTabActive;
  const styleActive = !settingsOpen && (styleOpen || styleTabActive);

  return (
    <nav
      className="ol-mobile-bottom-nav"
      style={{
        position: 'absolute',
        left: 0,
        right: 0,
        bottom: 0,
        zIndex: 55,
        display: 'flex',
        alignItems: 'stretch',
        justifyContent: 'space-around',
        gap: 4,
        padding: '6px 8px calc(6px + env(safe-area-inset-bottom, 0px))',
        borderTop: '0.5px solid var(--ol-line-soft)',
        background: 'var(--ol-surface)',
      }}
    >
      {MOBILE_BOTTOM_TABS.map((tab) => {
        const active = !settingsOpen && currentTab === tab.id;
        return (
          <button
            key={tab.id}
            type="button"
            onClick={() => onSelectTab(tab.id)}
            className={active ? 'ol-nav-btn ol-nav-btn-active' : 'ol-nav-btn'}
            style={mobileNavBtnStyle}
          >
            <Icon name={tab.icon} size={18} />
            <span
              style={{
                fontSize: 10.5,
                fontWeight: active
                  ? 'var(--ol-font-weight-semibold)'
                  : 'var(--ol-font-weight-medium)',
              }}
            >
              {t(`nav.${tab.id}`)}
            </span>
          </button>
        );
      })}
      <button
        type="button"
        onClick={onOpenStyle}
        className={styleActive ? 'ol-nav-btn ol-nav-btn-active' : 'ol-nav-btn'}
        style={mobileNavBtnStyle}
      >
        <Icon name="style" size={18} />
        <span
          style={{
            fontSize: 10.5,
            fontWeight: styleActive
              ? 'var(--ol-font-weight-semibold)'
              : 'var(--ol-font-weight-medium)',
          }}
        >
          {t('nav.group.style')}
        </span>
      </button>
      <button
        type="button"
        onClick={onOpenMore}
        className={moreActive ? 'ol-nav-btn ol-nav-btn-active' : 'ol-nav-btn'}
        style={mobileNavBtnStyle}
      >
        <Icon name="more" size={18} />
        <span
          style={{
            fontSize: 10.5,
            fontWeight: moreActive
              ? 'var(--ol-font-weight-semibold)'
              : 'var(--ol-font-weight-medium)',
          }}
        >
          {t('nav.more')}
        </span>
      </button>
    </nav>
  );
}

const mobileNavBtnStyle: CSSProperties = {
  flex: 1,
  minWidth: 0,
  display: 'flex',
  flexDirection: 'column',
  alignItems: 'center',
  justifyContent: 'center',
  gap: 4,
  padding: '6px 4px',
  border: 0,
  borderRadius: 10,
  background: 'transparent',
  fontFamily: 'inherit',
  cursor: 'default',
};

function ProviderSetupPrompt({
  closing = false,
  onLater,
  onOpenSettings,
  onRestore,
}: {
  closing?: boolean;
  onLater: () => void;
  onOpenSettings: () => void;
  onRestore: () => void;
}) {
  const { t } = useTranslation();
  const overlayRef = useRef<HTMLDivElement>(null);
  const cardRef = useRef<HTMLDivElement>(null);
  useOverlayMotion(overlayRef, closing, 'backdrop');
  useOverlayMotion(cardRef, closing);
  return (
    <div
      ref={overlayRef}
      style={{
        position: 'absolute',
        inset: 0,
        zIndex: 70,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        padding: 28,
        background: 'var(--ol-dialog-backdrop)',
        backdropFilter: 'blur(6px) saturate(140%)',
        WebkitBackdropFilter: 'blur(6px) saturate(140%)',
        pointerEvents: closing ? 'none' : undefined,
      }}
    >
      <div
        ref={cardRef}
        style={{
          width: 360,
          borderRadius: 'var(--ol-dialog-radius)',
          background: 'var(--ol-surface)',
          border: '1px solid var(--ol-dialog-border)',
          boxShadow: 'var(--ol-dialog-shadow)',
          padding: 20,
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginBottom: 12 }}>
          <div
            style={{
              width: 34,
              height: 34,
              borderRadius: 8,
              background: 'rgba(37,99,235,0.10)',
              color: 'var(--ol-blue)',
              display: 'inline-flex',
              alignItems: 'center',
              justifyContent: 'center',
              flexShrink: 0,
            }}
          >
            <Icon name="settings" size={17} />
          </div>
          <div
            style={{
              fontSize: 14,
              fontWeight: 'var(--ol-font-weight-semibold)',
              color: 'var(--ol-ink)',
            }}
          >
            {t('shell.providerPrompt.title')}
          </div>
        </div>
        <div style={{ fontSize: 12.5, color: 'var(--ol-ink-3)', lineHeight: 1.55 }}>
          {t('shell.providerPrompt.body')}
        </div>
        <button
          type="button"
          className="ol-tool-button"
          onClick={onRestore}
          style={{ marginTop: 16, width: '100%' }}
        >
          {t('cloudSync.restore')}
        </button>
        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8, marginTop: 18 }}>
          <button
            onClick={onLater}
            style={{
              height: 32,
              padding: '0 13px',
              borderRadius: 8,
              border: '0.5px solid var(--ol-line-strong)',
              background: 'var(--ol-surface)',
              color: 'var(--ol-ink-3)',
              fontFamily: 'inherit',
              fontSize: 12.5,
              fontWeight: 'var(--ol-font-weight-medium)',
              cursor: 'default',
              transition:
                'background 0.16s var(--ol-motion-quick), border-color 0.16s var(--ol-motion-quick)',
            }}
          >
            {t('shell.providerPrompt.later')}
          </button>
          <button
            onClick={onOpenSettings}
            style={{
              height: 32,
              padding: '0 14px',
              borderRadius: 8,
              border: 0,
              background: 'var(--ol-primary-solid-bg)',
              color: 'var(--ol-primary-solid-ink)',
              fontFamily: 'inherit',
              fontSize: 12.5,
              fontWeight: 'var(--ol-font-weight-medium)',
              cursor: 'default',
              transition:
                'background 0.16s var(--ol-motion-quick), transform 0.12s var(--ol-motion-quick)',
            }}
          >
            {t('shell.providerPrompt.openSettings')}
          </button>
        </div>
      </div>
    </div>
  );
}

function HotkeyModeMigrationPrompt({
  closing = false,
  onLater,
  onOpenSettings,
}: {
  closing?: boolean;
  onLater: () => void;
  onOpenSettings: () => void;
}) {
  const { t } = useTranslation();
  const overlayRef = useRef<HTMLDivElement>(null);
  const cardRef = useRef<HTMLDivElement>(null);
  useOverlayMotion(overlayRef, closing, 'backdrop');
  useOverlayMotion(cardRef, closing);
  return (
    <div
      ref={overlayRef}
      style={{
        position: 'absolute',
        inset: 0,
        zIndex: 70,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        padding: 28,
        background: 'var(--ol-dialog-backdrop)',
        backdropFilter: 'blur(6px) saturate(140%)',
        WebkitBackdropFilter: 'blur(6px) saturate(140%)',
        pointerEvents: closing ? 'none' : undefined,
      }}
    >
      <div
        ref={cardRef}
        style={{
          width: 380,
          borderRadius: 'var(--ol-dialog-radius)',
          background: 'var(--ol-surface)',
          border: '1px solid var(--ol-dialog-border)',
          boxShadow: 'var(--ol-dialog-shadow)',
          padding: 20,
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginBottom: 12 }}>
          <div
            style={{
              width: 34,
              height: 34,
              borderRadius: 8,
              background: 'rgba(37,99,235,0.10)',
              color: 'var(--ol-blue)',
              display: 'inline-flex',
              alignItems: 'center',
              justifyContent: 'center',
              flexShrink: 0,
            }}
          >
            <Icon name="mic" size={17} />
          </div>
          <div
            style={{
              fontSize: 14,
              fontWeight: 'var(--ol-font-weight-semibold)',
              color: 'var(--ol-ink)',
            }}
          >
            {t('shell.hotkeyModePrompt.title')}
          </div>
        </div>
        <div style={{ fontSize: 12.5, color: 'var(--ol-ink-3)', lineHeight: 1.55 }}>
          {t('shell.hotkeyModePrompt.body')}
        </div>
        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8, marginTop: 18 }}>
          <button
            onClick={onLater}
            style={{
              height: 32,
              padding: '0 13px',
              borderRadius: 8,
              border: '0.5px solid var(--ol-line-strong)',
              background: 'var(--ol-surface)',
              color: 'var(--ol-ink-3)',
              fontFamily: 'inherit',
              fontSize: 12.5,
              fontWeight: 'var(--ol-font-weight-medium)',
              cursor: 'default',
              transition:
                'background 0.16s var(--ol-motion-quick), border-color 0.16s var(--ol-motion-quick)',
            }}
          >
            {t('shell.hotkeyModePrompt.later')}
          </button>
          <button
            onClick={onOpenSettings}
            style={{
              height: 32,
              padding: '0 14px',
              borderRadius: 8,
              border: 0,
              background: 'var(--ol-primary-solid-bg)',
              color: 'var(--ol-primary-solid-ink)',
              fontFamily: 'inherit',
              fontSize: 12.5,
              fontWeight: 'var(--ol-font-weight-medium)',
              cursor: 'default',
              transition:
                'background 0.16s var(--ol-motion-quick), transform 0.12s var(--ol-motion-quick)',
            }}
          >
            {t('shell.hotkeyModePrompt.openSettings')}
          </button>
        </div>
      </div>
    </div>
  );
}
