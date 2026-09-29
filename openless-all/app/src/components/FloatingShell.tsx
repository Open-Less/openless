// Main window shell: sidebar navigation, page switching, mobile panels, and the
// settings modal. Pages share preference state; business reads/writes go to Core via typed IPC.

import { useEffect, useRef, useState, type ComponentType, type CSSProperties } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from './Icon';
import { Tooltip } from './Tooltip';
import { WindowChrome, detectOS, type OS } from './WindowChrome';
import { AudioCueListener } from './AudioCue';
import { SettingsModal } from './SettingsModal';
import { CloudSyncSetupPrompt } from './CloudSyncSetupPrompt';
import { Overview } from '../pages/Overview';
import { History } from '../pages/History';
import { Vocab } from '../pages/Vocab';
import { Style } from '../pages/Style';
import { Marketplace } from '../pages/Marketplace';
import { Translation } from '../pages/Translation';
import { SelectionAsk } from '../pages/SelectionAsk';
import { QuickNote } from '../pages/QuickNote';
import { Corrections } from '../pages/Corrections';
import { IS_BETA_BUILD } from '../lib/appVersion';
import {
  HOTKEY_MODE_MIGRATION_ACK_KEY,
  HOTKEY_MODE_MIGRATION_DEFERRED_KEY,
  shouldShowHotkeyModeMigrationPrompt,
} from '../lib/hotkeyMigration';
import { applyFontScale, readFontScale } from '../lib/fontScale';
import { useExitMount } from '../lib/useExitMount';
import { useOverlayMotion, usePageTransition } from '../lib/motion';
import { getCredentials } from '../lib/ipc';
import {
  PROVIDER_SETUP_PROMPT_DEFERRED_KEY,
  shouldShowProviderSetupPrompt,
} from '../lib/providerSetup';
import { type SettingsSectionId } from './SettingsModal';
import { MobileMoreSheet } from './MobileMoreSheet';
import { MobileStyleSheet } from './MobileStyleSheet';
import { subItemLabelKey } from '../lib/navLabels';
import { applyStackedLayoutFromPrefs } from '../lib/stackedLayout';
import { applyConservativeLayout } from '../lib/conservativeLayout';
import { useMobileLayout, useConservativeLayout } from '../lib/useMobileLayout';
import { useHotkeySettings } from '../state/HotkeySettingsContext';
import { useAppState, type AppTab } from '../state/useAppState';

const MORE_TAB_IDS: AppTab[] = ['vocab', 'translation', 'selectionAsk', 'quickNote', 'corrections'];
const STYLE_TAB_IDS: AppTab[] = ['style', 'marketplace'];

/** Reserve the native traffic-light strip before the sidebar's version row. */
const MAC_TRAFFIC_LIGHT_CLEARANCE = 44;
const SIDEBAR_WIDTH = 226;

/** tab → page component map (renders main content; decoupled from the sidebar tree, includes pages not listed there directly). */
const PAGE_CMP: Record<Exclude<AppTab, 'localAsr'>, ComponentType> = {
  overview: Overview,
  history: History,
  vocab: Vocab,
  style: Style,
  marketplace: Marketplace,
  translation: Translation,
  selectionAsk: SelectionAsk,
  quickNote: QuickNote,
  corrections: Corrections,
};

/** Main nav: direct entries and expandable groups; page components resolve through PAGE_CMP. */
type NavNode =
  | { kind: 'item'; id: AppTab; icon: string }
  | { kind: 'group'; key: string; icon: string; children: Array<{ id: AppTab }> };

const NAV_TREE: NavNode[] = [
  { kind: 'item', id: 'overview', icon: 'overview' },
  { kind: 'item', id: 'history', icon: 'history' },
  { kind: 'item', id: 'vocab', icon: 'vocab' },
  {
    kind: 'group',
    key: 'style',
    icon: 'style',
    children: [{ id: 'style' }, { id: 'marketplace' }],
  },
  {
    kind: 'group',
    key: 'tools',
    icon: 'selectionAsk',
    children: [
      { id: 'translation' },
      { id: 'selectionAsk' },
      { id: 'quickNote' },
      { id: 'corrections' },
    ],
  },
];

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
  const os = osProp ?? detectOS();
  return (
    <WindowChrome os={os} title="OpenLess" height="100%">
      <FloatingShellBody os={os} initialTab={initialTab} initialSettings={initialSettings} />
    </WindowChrome>
  );
}

function FloatingShellBody({
  os,
  initialTab,
  initialSettings,
}: {
  os: OS;
  initialTab: AppTab;
  initialSettings: boolean;
}) {
  const { t } = useTranslation();
  const mobile = useMobileLayout();
  const conservative = useConservativeLayout();
  const { prefs } = useHotkeySettings();
  const { currentTab, setCurrentTab, settingsOpen, setSettingsOpen } = useAppState(
    initialTab,
    initialSettings,
  );
  const [settingsInitialSection, setSettingsInitialSection] = useState<
    SettingsSectionId | undefined
  >();
  const [providerPromptOpen, setProviderPromptOpen] = useState(false);
  const [hotkeyModePromptOpen, setHotkeyModePromptOpen] = useState(false);
  const settingsMount = useExitMount(settingsOpen);
  const providerPromptMount = useExitMount(providerPromptOpen);
  const hotkeyPromptMount = useExitMount(hotkeyModePromptOpen);
  const [moreOpen, setMoreOpen] = useState(false);
  const [styleOpen, setStyleOpen] = useState(false);
  const shellRef = useRef<HTMLDivElement>(null);

  // The dialog records and moves focus before its background becomes inert.
  useEffect(() => {
    if (shellRef.current) shellRef.current.inert = settingsMount.mounted;
  }, [settingsMount.mounted]);

  const pageRef = useRef<HTMLDivElement>(null);
  const displayTab = usePageTransition(currentTab, pageRef, mobile);

  // Font scale — applied once from localStorage at startup; later changes come from
  // the Settings "personalization" section.
  useEffect(() => {
    applyFontScale(readFontScale());
  }, []);

  useEffect(() => {
    applyStackedLayoutFromPrefs(prefs?.stackedRowLayout);
    applyConservativeLayout(prefs?.conservativeLayout === true);
  }, [prefs?.stackedRowLayout, prefs?.conservativeLayout]);

  const Page = PAGE_CMP[displayTab as Exclude<AppTab, 'localAsr'>] ?? Overview;

  // Group expansion: default-expand the group containing the current page; the user
  // toggles via group titles.
  const groupOfTab = (tab: AppTab): string | null => {
    for (const node of NAV_TREE) {
      if (node.kind === 'group' && node.children.some((c) => c.id === tab)) return node.key;
    }
    return null;
  };
  const [openGroups, setOpenGroups] = useState<Record<string, boolean>>(() => {
    const active = groupOfTab(currentTab);
    return active ? { [active]: true } : {};
  });
  // Auto-expand the group when landing on one of its pages (e.g. jumping straight to
  // marketplace from the capsule).
  useEffect(() => {
    const active = groupOfTab(currentTab);
    if (active) setOpenGroups((prev) => (prev[active] ? prev : { ...prev, [active]: true }));
  }, [currentTab]);
  const toggleGroup = (key: string) => setOpenGroups((prev) => ({ ...prev, [key]: !prev[key] }));

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

  const openSettings = (section?: SettingsSectionId) => {
    setSettingsInitialSection(section);
    setSettingsOpen(true);
    setMoreOpen(false);
    setStyleOpen(false);
  };

  // Platform-appropriate settings shortcut.
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if ((os === 'mac' ? e.metaKey : e.ctrlKey) && e.key === ',') {
        e.preventDefault();
        openSettings();
      }
    };
    window.addEventListener('keydown', onKeyDown, true);
    return () => window.removeEventListener('keydown', onKeyDown, true);
  }, [os]);

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
  const moreTabActive = MORE_TAB_IDS.includes(currentTab);
  const styleTabActive = STYLE_TAB_IDS.includes(currentTab);

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
        <div
          ref={(element) => {
            if (element) element.inert = settingsOpen;
          }}
          style={{ display: 'contents' }}
        >
          <MobileTopBar
            title={mobileTitle}
            onOpenSettings={() => openSettings()}
            settingsActive={settingsOpen}
          />
        </div>
      )}

      {/* Main shell — flush with the frosted backplate (no separate float). */}
      <div
        ref={shellRef}
        data-ol-settings-open={settingsOpen ? 'true' : undefined}
        className="ol-app-shell-bg"
        style={{
          flex: 1,
          minHeight: 0,
          display: 'flex',
          overflow: 'hidden',
          position: 'relative',
          zIndex: 1,
        }}
      >
        {/* Sidebar — desktop / wide only. */}
        {!mobile && (
          <aside
            className="ol-sidebar-surface"
            style={{
              width: SIDEBAR_WIDTH,
              height: '100%',
              flexShrink: 0,
              display: 'flex',
              flexDirection: 'column',
              background: 'var(--ol-sidebar-bg)',
              borderRight: '0.5px solid var(--ol-line)',
              // mac: clear the traffic-light height at the top so brand/nav sit below it.
              padding:
                os === 'mac' ? `${MAC_TRAFFIC_LIGHT_CLEARANCE}px 10px 12px` : '10px 10px 12px',
            }}
          >
            {/* Version row: replaces the old "OpenLess" brand spot, filling the gap
              above the nav; the BETA badge shares the version number's baseline. */}
            <div
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 8,
                flexWrap: 'wrap',
                padding: '0 10px 12px',
                fontFamily: 'var(--ol-font-sans)',
                fontSize: 12,
                color: 'var(--ol-ink-4)',
              }}
            >
              {IS_BETA_BUILD && (
                <span
                  style={{
                    display: 'inline-flex',
                    alignItems: 'center',
                    padding: '1px 6px',
                    fontSize: 10,
                    lineHeight: '14px',
                    fontWeight: 600,
                    letterSpacing: '0.05em',
                    textTransform: 'uppercase',
                    color: 'var(--ol-blue)',
                    background: 'transparent',
                    border: '0.5px solid var(--ol-pill-blue-border)',
                    borderRadius: 5,
                  }}
                >
                  {t('shell.betaTag')}
                </span>
              )}
            </div>

            {/* nav — flat items + expandable groups. Flat: overview/history/vocab.
              Groups: Style (polish modes + marketplace) / Tools (selection ask + translation).
              Active items get a static surface-2 rounded bg via the .ol-nav-btn-active
              class; group titles expand/collapse on click. */}
            <nav style={{ display: 'flex', flexDirection: 'column', gap: 1 }}>
              {NAV_TREE.map((node) => {
                if (node.kind === 'item') {
                  const active = !settingsOpen && currentTab === node.id;
                  return (
                    <Tooltip
                      key={node.id}
                      content={t(`shell.navHint.${node.id}`)}
                      placement="right"
                    >
                      <button
                        onClick={() => setCurrentTab(node.id)}
                        className={active ? 'ol-nav-btn ol-nav-btn-active' : 'ol-nav-btn'}
                        style={navBtnStyle}
                      >
                        <Icon name={node.icon} size={16} />
                        <span style={{ flex: 1 }}>{t(`nav.${node.id}`)}</span>
                      </button>
                    </Tooltip>
                  );
                }
                const expanded = !!openGroups[node.key];
                const groupActive = !settingsOpen && node.children.some((c) => c.id === currentTab);
                return (
                  <div key={node.key}>
                    {/* Group title: click only expands/collapses, never navigates. */}
                    <button
                      onClick={() => toggleGroup(node.key)}
                      className={
                        groupActive && !expanded ? 'ol-nav-btn ol-nav-btn-active' : 'ol-nav-btn'
                      }
                      aria-expanded={expanded}
                      style={navBtnStyle}
                    >
                      <Icon name={node.icon} size={16} />
                      <span style={{ flex: 1 }}>{t(`nav.group.${node.key}`)}</span>
                      <Icon
                        name="chevRight"
                        size={12}
                        style={{
                          color: 'var(--ol-ink-4)',
                          flexShrink: 0,
                          transform: expanded ? 'rotate(90deg)' : 'rotate(0deg)',
                          transition: 'transform 0.20s var(--ol-motion-spring)',
                        }}
                      />
                    </button>
                    {/* Children: the grid-rows 0fr↔1fr transition gives an expand
                        animation without hardcoded heights. */}
                    <div
                      style={{
                        display: 'grid',
                        gridTemplateRows: expanded ? '1fr' : '0fr',
                        transition: 'grid-template-rows 0.24s var(--ol-motion-spring)',
                      }}
                    >
                      <div style={{ overflow: 'hidden', minHeight: 0 }}>
                        <div
                          style={{
                            display: 'flex',
                            flexDirection: 'column',
                            gap: 1,
                            padding: '1px 0 2px',
                          }}
                        >
                          {node.children.map((child) => {
                            const active = !settingsOpen && currentTab === child.id;
                            return (
                              <button
                                key={child.id}
                                onClick={() => setCurrentTab(child.id)}
                                className={
                                  active
                                    ? 'ol-nav-btn ol-nav-subitem ol-nav-btn-active'
                                    : 'ol-nav-btn ol-nav-subitem'
                                }
                                tabIndex={expanded ? 0 : -1}
                                style={{ ...navBtnStyle, paddingLeft: 30 }}
                              >
                                <span style={{ flex: 1 }}>{t(subItemLabelKey(child.id))}</span>
                              </button>
                            );
                          })}
                        </div>
                      </div>
                    </div>
                  </div>
                );
              })}
            </nav>

            <div style={{ flex: 1 }} />

            {/* Footer holds only the settings button. */}
            <div style={{ display: 'flex', flexDirection: 'column', gap: 8, paddingTop: 10 }}>
              <Tooltip content={t('shell.navHint.settings')} placement="right">
                <button
                  onClick={() => openSettings()}
                  className={settingsOpen ? 'ol-nav-btn ol-nav-btn-active' : 'ol-nav-btn'}
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    gap: 10,
                    padding: '8px 10px',
                    borderRadius: 8,
                    border: 0,
                    fontFamily: 'inherit',
                    fontSize: 15,
                    cursor: 'default',
                    transition:
                      'color 0.16s var(--ol-motion-quick), background 0.16s var(--ol-motion-quick)',
                    textAlign: 'left',
                  }}
                >
                  <Icon name="settings" size={16} />
                  <span style={{ flex: 1 }}>{t('shell.footer.settings')}</span>
                </button>
              </Tooltip>
            </div>
          </aside>
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
            <div
              ref={pageRef}
              key={displayTab}
              data-ol-page={displayTab}
              // issue #243: all tabs allow overflow:auto so bottom content stays
              //   reachable when the window shrinks or copy grows (Codex P1: overview
              //   used hidden, leaving the Recent card fully invisible after shrinking).
              //   - Overview uses its internal flex to grow the bottom row to fill;
              //     at normal size the content fits exactly → no scrollbar; a thin
              //     scrollbar appears only when it truly overflows.
              //   - Other tabs use the thin scrollbar too.
              className="ol-thinscroll ol-scroll-fade"
              // Overview is a fixed single-screen page (overflow hidden, never scrolls):
              // no scrollbar gutter reserved.
              data-ol-page-fixed={displayTab === 'overview' && !mobile ? 'true' : undefined}
              style={{
                flex: 1,
                minHeight: 0,
                // Overview is a fixed single-screen page: no scrolling; all dashboards
                // fit one screen with heights distributed by Overview.tsx's internal
                // flex; other pages keep the thin scrollbar.
                overflow: displayTab === 'overview' && !mobile ? 'hidden' : 'auto',
                padding: mobile
                  ? '16px 16px calc(16px + env(safe-area-inset-bottom, 0px) + 56px)'
                  : displayTab === 'overview'
                    ? `${os === 'mac' ? 80 : 56}px 28px 24px`
                    : `${os === 'mac' ? 80 : 56}px 28px 32px`,
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
            </div>
          </main>
        </div>
      </div>

      {mobile && (
        <>
          {!settingsOpen && !styleOpen && !moreOpen && (
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
        <SettingsModal
          key={settingsInitialSection ?? 'default'}
          os={os}
          closing={settingsMount.closing}
          initialSettingsSection={settingsInitialSection}
          onClose={() => setSettingsOpen(false)}
        />
      )}

      {providerPromptMount.mounted ? (
        <ProviderSetupPrompt
          closing={providerPromptMount.closing}
          onLater={rememberProviderPrompt}
          onOpenSettings={openProviderSettings}
          onRestore={() => {
            rememberProviderPrompt();
            openSettings('privacy');
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
        blocked={settingsMount.mounted || providerPromptMount.mounted || hotkeyPromptMount.mounted}
        onSetup={() => openSettings('privacy')}
      />
      <AudioCueListener />

      {/* Enter keyframes shared by tab switching, the provider prompt, and the footer popover */}
      <style>{`
        /* Nav three-tier visual hierarchy (flat, no pill slider):
             base   → ink-3 (mid-gray text + transparent bg)
             hover  → ink (dark text + surface-2 light bg)  ← highlights labels on hover
             active → ink (bold dark text + static surface-2 rounded bg)
           All via class: sidebar buttons carry no inline background so :hover / active
           backgrounds can apply (CSS can't override inline style); other .ol-nav-btn
           consumers keep their inline background, so the active bg doesn't affect them. */
        .ol-nav-btn {
          color: var(--ol-ink-3);
          font-weight: 500;
        }
        .ol-nav-btn.ol-nav-btn-active {
          color: var(--ol-ink);
          font-weight: 600;
          background: var(--ol-surface-2);
        }
        .ol-nav-btn:not(.ol-nav-btn-active):hover {
          background: var(--ol-surface-2);
          color: var(--ol-ink);
        }
      `}</style>
    </div>
  );
}

const MOBILE_BOTTOM_TABS: Array<{ id: AppTab; icon: string }> = [
  { id: 'overview', icon: 'overview' },
  { id: 'history', icon: 'history' },
];

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
            fontWeight: 600,
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
            <span style={{ fontSize: 10.5, fontWeight: active ? 600 : 500 }}>
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
        <span style={{ fontSize: 10.5, fontWeight: styleActive ? 600 : 500 }}>
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
        <span style={{ fontSize: 10.5, fontWeight: moreActive ? 600 : 500 }}>{t('nav.more')}</span>
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

/** Base style for sidebar nav buttons (shared by flat items / group titles / children; children override paddingLeft). */
const navBtnStyle: CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  gap: 10,
  width: '100%',
  padding: '8px 10px',
  borderRadius: 8,
  border: 0,
  // (After the Codex sidebar): 15px nav text, 16px icons — closer to the reference design.
  fontFamily: 'inherit',
  fontSize: 15,
  cursor: 'default',
  transition: 'color 0.16s var(--ol-motion-quick), background 0.16s var(--ol-motion-quick)',
  textAlign: 'left',
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
          <div style={{ fontSize: 14, fontWeight: 600, color: 'var(--ol-ink)' }}>
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
              fontWeight: 500,
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
              fontWeight: 500,
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
          <div style={{ fontSize: 14, fontWeight: 600, color: 'var(--ol-ink)' }}>
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
              fontWeight: 500,
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
              fontWeight: 500,
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
