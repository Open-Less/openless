// FloatingShell.tsx — frosted outer frame + raised inner console.
// Sidebar lives INSIDE the console card.
// Settings opens as a centered modal sheet from the sidebar bottom entry.
//
// Ported verbatim from design_handoff_openless/variants.jsx::FloatingShell.

import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ComponentType, type CSSProperties } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from './Icon';
import { WindowChrome, detectOS, type OS } from './WindowChrome';
import { AudioCueListener } from "./AudioCue";
import { SettingsModal } from './SettingsModal';
import { Overview } from '../pages/Overview';
import { History } from '../pages/History';
import { Vocab } from '../pages/Vocab';
import { Style } from '../pages/Style';
import { Translation } from '../pages/Translation';
import { SelectionAsk } from '../pages/SelectionAsk';
// 风格市场不再作为独立 nav tab —— 已整合为 Style 页面内 modal（入口在「风格包」标题右侧）。
// LocalAsr 不再作为主 nav tab——本地 ASR 模型管理已合并到 Settings → Advanced 中
// 通过 <LocalAsr embedded /> 渲染。这里之前的 import 与 NAV_BASE 条目都已移除。
import { APP_VERSION_LABEL, IS_BETA_BUILD } from '../lib/appVersion';
import {
  HOTKEY_MODE_MIGRATION_ACK_KEY,
  HOTKEY_MODE_MIGRATION_DEFERRED_KEY,
  shouldShowHotkeyModeMigrationPrompt,
} from '../lib/hotkeyMigration';
import { applyFontScale, readFontScale } from '../lib/fontScale';
import { getCredentials } from '../lib/ipc';
import {
  PROVIDER_SETUP_PROMPT_DEFERRED_KEY,
  shouldShowProviderSetupPrompt,
} from '../lib/providerSetup';
import { type SettingsSectionId } from './SettingsModal';
import { MobileMoreSheet } from './MobileMoreSheet';
import { useMobileLayout } from '../lib/useMobileLayout';
import { useAppState, type AppTab } from '../state/useAppState';

const MORE_TAB_IDS: AppTab[] = ['vocab', 'translation', 'selectionAsk'];

interface NavItem {
  id: AppTab;
  name: string;
  icon: string;
  cmp: ComponentType;
}

const NAV_BASE: Array<Omit<NavItem, 'name'>> = [
  { id: 'overview', icon: 'overview', cmp: Overview },
  { id: 'history', icon: 'history', cmp: History },
  { id: 'vocab', icon: 'vocab', cmp: Vocab },
  { id: 'style', icon: 'style', cmp: Style },
  { id: 'translation', icon: 'translate', cmp: Translation },
  { id: 'selectionAsk', icon: 'selectionAsk', cmp: SelectionAsk },
];

interface FloatingShellProps {
  os?: OS;
  initialTab?: AppTab;
  initialSettings?: boolean;
}

export function FloatingShell({ os: osProp, initialTab = 'overview', initialSettings = false }: FloatingShellProps) {
  const os = osProp ?? detectOS();
  return (
    <WindowChrome os={os} title="OpenLess" height="100%">
      <FloatingShellBody os={os} initialTab={initialTab} initialSettings={initialSettings} />
    </WindowChrome>
  );
}

function FloatingShellBody({ os, initialTab, initialSettings }: { os: OS; initialTab: AppTab; initialSettings: boolean }) {
  const { t } = useTranslation();
  const mobile = useMobileLayout();
  const { currentTab, setCurrentTab, settingsOpen, setSettingsOpen } = useAppState(initialTab, initialSettings);
  const [settingsInitialSection, setSettingsInitialSection] = useState<SettingsSectionId | undefined>();
  const [providerPromptOpen, setProviderPromptOpen] = useState(false);
  const [hotkeyModePromptOpen, setHotkeyModePromptOpen] = useState(false);
  const [moreOpen, setMoreOpen] = useState(false);

  // tab 切换的 cross-fade：旧页 blur+fade out（180ms），结束后挂载新页（走 ol-page-slide enter）。
  // displayTab 是实际渲染的 tab，currentTab 是用户点中的目标 tab。
  const [displayTab, setDisplayTab] = useState<AppTab>(initialTab);
  const [tabPhase, setTabPhase] = useState<'idle' | 'exiting'>('idle');
  useEffect(() => {
    if (currentTab === displayTab) return;
    setTabPhase('exiting');
    const id = window.setTimeout(() => {
      setDisplayTab(currentTab);
      setTabPhase('idle');
    }, 180);
    return () => window.clearTimeout(id);
  }, [currentTab, displayTab]);

  // 字体档位 — 启动时按 localStorage 应用一次；之后改动来自 Settings 的"个性化"section。
  useEffect(() => {
    applyFontScale(readFontScale());
  }, []);

  const NAV = useMemo<NavItem[]>(
    () => NAV_BASE.map(b => ({ ...b, name: t(`nav.${b.id}`) })),
    [t],
  );
  const Page = (NAV.find((n) => n.id === displayTab) ?? NAV[0]).cmp;

  // sidebar nav 滑动指示器：测量当前 active button 的 offsetTop / height，
  // 用一个 absolute pill 平滑滑过去，而不是每个按钮各自瞬切背景色。
  const navItemRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const [pillRect, setPillRect] = useState<{ top: number; height: number } | null>(null);
  useLayoutEffect(() => {
    if (settingsOpen) {
      setPillRect(null);
      return;
    }
    const idx = NAV.findIndex(n => n.id === currentTab);
    if (idx < 0) {
      setPillRect(null);
      return;
    }
    const el = navItemRefs.current[idx];
    if (!el) return;
    setPillRect({ top: el.offsetTop, height: el.offsetHeight });
  }, [currentTab, settingsOpen, NAV]);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      const credentials = await getCredentials();
      const promptDeferredValue = window.sessionStorage.getItem(PROVIDER_SETUP_PROMPT_DEFERRED_KEY);
      if (!cancelled && shouldShowProviderSetupPrompt(credentials, promptDeferredValue)) {
        setProviderPromptOpen(true);
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

  // 之前监听的 NAVIGATE_LOCAL_ASR_EVENT 已无意义——「模型设置」独立 tab 已下线，
  // 模型管理 UI 现在通过 Settings → Advanced 的 <LocalAsr embedded /> 渲染，
  // 用户在 Settings 内即可一站式管理，无需跨页跳转。

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
  };

  // ⌘, 打开设置页面
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.metaKey && e.key === ',') {
        e.preventDefault();
        openSettings();
      }
    };
    window.addEventListener('keydown', onKeyDown, true);
    return () => window.removeEventListener('keydown', onKeyDown, true);
  }, []);

  const openProviderSettings = () => {
    rememberProviderPrompt();
    openSettings('services');
  };

  const openHotkeyRecordingSettings = () => {
    window.localStorage.setItem(HOTKEY_MODE_MIGRATION_ACK_KEY, '1');
    setHotkeyModePromptOpen(false);
    openSettings('general');
  };

  const mobileTitle = settingsOpen
    ? t('shell.footer.settings')
    : (NAV.find(n => n.id === currentTab)?.name ?? t('nav.overview'));
  const moreTabActive = MORE_TAB_IDS.includes(currentTab);
  const useOpaqueMain = mobile || os === 'linux';

  return (
    <div style={{ flex: 1, position: 'relative', display: 'flex', flexDirection: 'column', minHeight: 0, paddingTop: mobile ? 0 : os === 'mac' ? 28 : 0 }}>

      {mobile && (
        <MobileTopBar
          title={mobileTitle}
          onOpenSettings={() => openSettings()}
          settingsActive={settingsOpen}
        />
      )}

      {/* Main shell — flush with the frosted backplate (no separate float). */}
      <div
        className="ol-app-shell-bg"
        style={{
          flex: 1, minHeight: 0,
          display: 'flex',
          overflow: 'hidden',
          position: 'relative',
          zIndex: 1,
        }}>

        {/* Sidebar — desktop / wide only */}
        {!mobile && (
        <aside
          className="ol-aura-sidebar"
          style={{
            width: 188,
            flexShrink: 0,
            display: 'flex', flexDirection: 'column',
            background: 'transparent',
            padding: '10px 10px 12px',
          }}>

          {/* brand */}
          <div style={{ display: 'flex', alignItems: 'center', gap: 9, padding: '2px 8px 12px' }}>
            <img
              src="AppIcon.png"
              alt="OpenLess"
              style={{ width: 22, height: 22, borderRadius: 5, boxShadow: '0 1px 2px rgba(0,0,0,.1), 0 0 0 0.5px rgba(0,0,0,.06)' }} />

            <div style={{ fontSize: 13.5, fontWeight: 600, letterSpacing: '-0.01em', color: 'var(--ol-ink)' }}>OpenLess</div>
          </div>

          {/* nav — 滑动指示器：active pill 是 absolute 元素，currentTab 改变时 top/height
              过渡到目标按钮的位置，而非各按钮自己瞬切背景色。hover 灰底通过 .ol-nav-btn 的
              CSS :hover 规则实现，仅对非 active 项生效。 */}
          <nav style={{ position: 'relative', display: 'flex', flexDirection: 'column', gap: 1 }}>
            {pillRect && (
              <div
                aria-hidden
                style={{
                  position: 'absolute',
                  left: 0,
                  right: 0,
                  top: pillRect.top,
                  height: pillRect.height,
                  background: 'var(--ol-surface)',
                  borderRadius: 8,
                  boxShadow: '0 1px 2px rgba(0,0,0,.05), 0 0 0 0.5px rgba(0,0,0,.06)',
                  transition: 'top 0.36s var(--ol-motion-spring), height 0.36s var(--ol-motion-spring)',
                  pointerEvents: 'none',
                  zIndex: 0,
                }}
              />
            )}
            {NAV.map((n, i) => {
              const active = !settingsOpen && currentTab === n.id;
              return (
                <button
                  key={n.id}
                  ref={el => { navItemRefs.current[i] = el; }}
                  onClick={() => setCurrentTab(n.id)}
                  className={active ? 'ol-nav-btn ol-nav-btn-active' : 'ol-nav-btn'}
                  style={{
                    display: 'flex', alignItems: 'center', gap: 10,
                    padding: '7px 10px',
                    borderRadius: 8, border: 0,
                    background: 'transparent',
                    fontFamily: 'inherit', fontSize: 13,
                    cursor: 'default',
                    transition: 'color 0.16s var(--ol-motion-quick), background 0.16s var(--ol-motion-quick)',
                    textAlign: 'left',
                    position: 'relative',
                    zIndex: 1,
                  }}>

                  <Icon name={n.icon} size={14} />
                  <span style={{ flex: 1 }}>{n.name}</span>
                </button>
              );
            })}
          </nav>

          <div style={{ flex: 1 }} />

          {/* 底部两行：上行 = 版本 chip（含 BETA 标），下行 = 设置按钮。
              单行布局在窄 sidebar 下会把「设置」挤成两行竖字 + 版本糊一起；
              翻回两行同时把顺序反过来：设置真正落到最底，版本在它上面。 */}
          <div style={{ display: 'flex', flexDirection: 'column', gap: 8, paddingTop: 10 }}>
            <div
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 8,
                flexWrap: 'wrap',
                padding: '0 10px',
                fontFamily: 'var(--ol-font-sans)',
                fontSize: 11,
                color: 'var(--ol-ink-4)',
              }}
            >
              {IS_BETA_BUILD && (
                <span style={{
                  display: 'inline-block',
                  padding: '2px 8px',
                  fontSize: 10,
                  fontWeight: 600,
                  letterSpacing: '0.04em',
                  textTransform: 'uppercase',
                  color: 'var(--ol-blue)',
                  background: 'rgba(37,99,235,0.10)',
                  borderRadius: 999,
                }}>{t('shell.betaTag')}</span>
              )}

              <span>{t('shell.footer.version', { version: APP_VERSION_LABEL })}</span>
            </div>

            <button
              onClick={() => openSettings()}
              className={settingsOpen ? 'ol-nav-btn ol-nav-btn-active' : 'ol-nav-btn'}
              style={{
                display: 'flex', alignItems: 'center', gap: 10,
                padding: '7px 10px',
                borderRadius: 8, border: 0,
                background: settingsOpen ? 'var(--ol-surface)' : 'transparent',
                boxShadow: settingsOpen ? '0 1px 2px rgba(0,0,0,.05), 0 0 0 0.5px rgba(0,0,0,.06)' : 'none',
                fontFamily: 'inherit', fontSize: 13,
                cursor: 'default',
                transition: 'color 0.16s var(--ol-motion-quick), background 0.16s var(--ol-motion-quick)',
                textAlign: 'left',
              }}
            >
              <Icon name="settings" size={14} />
              <span style={{ flex: 1 }}>{t('shell.footer.settings')}</span>
            </button>
          </div>
        </aside>
        )}

        {/* Main content — Linux 禁用透明窗口后使用不透明面；mobile 全宽无玻璃层。 */}
        <div style={{ flex: 1, minWidth: 0, padding: mobile ? 0 : '4px 8px 8px 0', display: 'flex' }}>
          <main
            className="ol-console-main ol-aura-panel"
            style={{
              flex: 1, minWidth: 0,
              overflow: 'hidden',
              background: useOpaqueMain ? 'var(--ol-panel-bg)' : 'var(--ol-aura-surface)',
              backdropFilter: useOpaqueMain ? 'none' : 'blur(18px) saturate(170%)',
              WebkitBackdropFilter: useOpaqueMain ? 'none' : 'blur(18px) saturate(170%)',
              borderRadius: 'var(--ol-r-lg)',
              ...(mobile ? { borderRadius: 0 } : {}),
              border: mobile ? 'none' : '0.5px solid var(--ol-panel-border)',
              boxShadow: mobile ? 'none' : 'var(--ol-panel-shadow)',
              display: 'flex',
              flexDirection: 'column',
            }}
          >
            {/* key={displayTab} 让每次切换重挂这棵子树 → ol-page-slide keyframe 重新触发。
                旧 tab 退出时不立刻 unmount，而是先播 ol-page-fadeout（blur+淡出），
                180ms 后再切到新 tab 并播入场动画。详见 displayTab/tabPhase 的 effect。
                padding + overflow:auto 直接挂在这棵 wrapper 上：
                  - 自然高度的页（Overview / Vocab / Style）—— 整页内容超出时 wrapper 出现滚动条
                  - 用 height:100% 撑满的页（History 左右双列）—— 100% 能解析到 wrapper 的固定高度，
                    两列内部各自的 overflow:auto 才能独立滚动 */}
            <div
              key={displayTab}
              // issue #243：所有 tab 都允许 overflow:auto，让窗口被压缩 / 文案
              //   变长时仍可触达底部内容（Codex P1：之前 overview 用 hidden
              //   会让缩窗后 Recent 卡彻底不可见）。
              //   - Overview 借 Overview.tsx 内部 flex 把底部行 grow 到撑满，
              //     正常尺寸下内容刚好占满 → 浏览器自动不显示 scrollbar；
              //     真挤不下了才 fallback 出细滚动条。
              //   - 其他 tab 同样走细滚动条。
              className="ol-thinscroll"
              style={{
                flex: 1, minHeight: 0,
                overflow: 'auto',
                padding: mobile
                  ? '16px 16px calc(16px + env(safe-area-inset-bottom, 0px) + 56px)'
                  : '24px 28px 32px',
                // position:relative 让页面里的"已保存"toast 用 absolute top:16 right:16
                // 锚到这块控制台卡的右上角，而不是横在页头变成长横幅。
                position: 'relative',
                animation: mobile
                  ? (tabPhase === 'exiting'
                    ? 'ol-page-fadeout-mobile 0.18s var(--ol-motion-soft) forwards'
                    : 'ol-page-fade-mobile 0.22s var(--ol-motion-soft) both')
                  : (tabPhase === 'exiting'
                    ? 'ol-page-fadeout 0.18s var(--ol-motion-soft) forwards'
                    : 'ol-page-slide 0.34s var(--ol-motion-spring) both'),
                willChange: mobile ? 'opacity' : 'opacity, transform, filter',
                display: 'flex',
                flexDirection: 'column',
              }}
            >
              {displayTab === 'overview' ? (
                <Overview onOpenHistory={() => setCurrentTab('history')} />
              ) : (
                <Page />
              )}
            </div>
          </main>
        </div>
      </div>

      {mobile && (
        <>
          <MobileBottomNav
            currentTab={currentTab}
            moreOpen={moreOpen}
            moreTabActive={moreTabActive}
            settingsOpen={settingsOpen}
            onSelectTab={id => {
              setMoreOpen(false);
              setCurrentTab(id);
            }}
            onOpenMore={() => setMoreOpen(true)}
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

      {/* Settings modal — rendered inside this window */}
      {settingsOpen &&
        <SettingsModal
          key={settingsInitialSection ?? 'default'}
          os={os}
          initialSettingsSection={settingsInitialSection}
          onClose={() => setSettingsOpen(false)}
        />
      }

      {providerPromptOpen ? (
        <ProviderSetupPrompt
          onLater={rememberProviderPrompt}
          onOpenSettings={openProviderSettings}
        />
      ) : hotkeyModePromptOpen ? (
        <HotkeyModeMigrationPrompt
          onLater={deferHotkeyModePrompt}
          onOpenSettings={openHotkeyRecordingSettings}
        />
      ) : null}
      <AudioCueListener />

      {/* tab 切换 + provider prompt + footer popover 公用的入场关键帧 */}
      <style>{`
        /* nav 三段视觉层次：
             基础态  → ink-3（中灰文字 + 透明底）
             hover  → ink（深色文字 + 浅灰底）  ← 让"翻译"等字词在悬停时高亮，跟基础/选中都拉开差距
             选中  → ink（深色文字 + 白色 pill 底，由 absolute pill 提供）
           inline color/fontWeight 留给 active 项写最高优先级；非 active 走 class，
           这样 :hover 能正确覆盖（CSS 不能盖 inline style）。 */
        .ol-nav-btn {
          color: var(--ol-ink-3);
          font-weight: 500;
        }
        .ol-nav-btn.ol-nav-btn-active {
          color: var(--ol-ink);
          font-weight: 600;
        }
        .ol-nav-btn:not(.ol-nav-btn-active):hover {
          background: var(--ol-nav-hover-bg);
          color: var(--ol-ink);
        }
        @keyframes ol-page-slide {
          from { opacity: 0; transform: translate3d(10px, 0, 0) scale(.996); filter: blur(6px); }
          to   { opacity: 1; transform: translate3d(0, 0, 0) scale(1); filter: blur(0); }
        }
        @keyframes ol-page-fadeout {
          from { opacity: 1; filter: blur(0); }
          to   { opacity: 0; filter: blur(8px); }
        }
        @keyframes ol-page-fade-mobile {
          from { opacity: 0; }
          to   { opacity: 1; }
        }
        @keyframes ol-page-fadeout-mobile {
          from { opacity: 1; }
          to   { opacity: 0; }
        }
        @keyframes ol-mobile-sheet-backdrop {
          from { opacity: 0; }
          to   { opacity: 1; }
        }
        @keyframes ol-mobile-sheet-up {
          from { opacity: 0; transform: translate3d(0, 12px, 0); }
          to   { opacity: 1; transform: translate3d(0, 0, 0); }
        }
        @keyframes ol-prompt-fade {
          from { opacity: 0; backdrop-filter: blur(0); -webkit-backdrop-filter: blur(0); }
          to   { opacity: 1; backdrop-filter: blur(6px); -webkit-backdrop-filter: blur(6px); }
        }
        @keyframes ol-prompt-pop {
          from { opacity: 0; transform: translateY(6px) scale(.97); filter: blur(6px); }
          to   { opacity: 1; transform: translateY(0) scale(1); filter: blur(0); }
        }
      `}</style>
    </div>
  );
}

const MOBILE_BOTTOM_TABS: Array<{ id: AppTab; icon: string }> = [
  { id: 'overview', icon: 'overview' },
  { id: 'history', icon: 'history' },
  { id: 'style', icon: 'style' },
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
  settingsOpen,
  onSelectTab,
  onOpenMore,
}: {
  currentTab: AppTab;
  moreOpen: boolean;
  moreTabActive: boolean;
  settingsOpen: boolean;
  onSelectTab: (tab: AppTab) => void;
  onOpenMore: () => void;
}) {
  const { t } = useTranslation();
  const moreActive = moreOpen || moreTabActive;

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
      {MOBILE_BOTTOM_TABS.map(tab => {
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
            <span style={{ fontSize: 10.5, fontWeight: active ? 600 : 500 }}>{t(`nav.${tab.id}`)}</span>
          </button>
        );
      })}
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

function ProviderSetupPrompt({ onLater, onOpenSettings }: { onLater: () => void; onOpenSettings: () => void }) {
  const { t } = useTranslation();
  return (
    <div
      style={{
        position: 'absolute',
        inset: 0,
        zIndex: 70,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        padding: 28,
        background: 'rgba(15,17,22,0.28)',
        backdropFilter: 'blur(6px) saturate(140%)',
        WebkitBackdropFilter: 'blur(6px) saturate(140%)',
        animation: 'ol-prompt-fade 0.2s var(--ol-motion-soft)',
      }}
    >
      <div
        style={{
          width: 360,
          borderRadius: 12,
          background: 'var(--ol-surface)',
          border: '0.5px solid rgba(0,0,0,.08)',
          boxShadow: '0 24px 70px -24px rgba(15,17,22,.38), 0 0 0 0.5px rgba(0,0,0,.06)',
          padding: 20,
          animation: 'ol-prompt-pop 0.26s var(--ol-motion-spring)',
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
          <div style={{ fontSize: 14, fontWeight: 600, color: 'var(--ol-ink)' }}>{t('shell.providerPrompt.title')}</div>
        </div>
        <div style={{ fontSize: 12.5, color: 'var(--ol-ink-3)', lineHeight: 1.55 }}>
          {t('shell.providerPrompt.body')}
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
              transition: 'background 0.16s var(--ol-motion-quick), border-color 0.16s var(--ol-motion-quick)',
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
              transition: 'background 0.16s var(--ol-motion-quick), transform 0.12s var(--ol-motion-quick)',
            }}
          >
            {t('shell.providerPrompt.openSettings')}
          </button>
        </div>
      </div>
    </div>
  );
}

function HotkeyModeMigrationPrompt({ onLater, onOpenSettings }: { onLater: () => void; onOpenSettings: () => void }) {
  const { t } = useTranslation();
  return (
    <div
      style={{
        position: 'absolute',
        inset: 0,
        zIndex: 70,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        padding: 28,
        background: 'rgba(15,17,22,0.28)',
        backdropFilter: 'blur(6px) saturate(140%)',
        WebkitBackdropFilter: 'blur(6px) saturate(140%)',
        animation: 'ol-prompt-fade 0.2s var(--ol-motion-soft)',
      }}
    >
      <div
        style={{
          width: 380,
          borderRadius: 12,
          background: 'var(--ol-surface)',
          border: '0.5px solid rgba(0,0,0,.08)',
          boxShadow: '0 24px 70px -24px rgba(15,17,22,.38), 0 0 0 0.5px rgba(0,0,0,.06)',
          padding: 20,
          animation: 'ol-prompt-pop 0.26s var(--ol-motion-spring)',
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
          <div style={{ fontSize: 14, fontWeight: 600, color: 'var(--ol-ink)' }}>{t('shell.hotkeyModePrompt.title')}</div>
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
              transition: 'background 0.16s var(--ol-motion-quick), border-color 0.16s var(--ol-motion-quick)',
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
              transition: 'background 0.16s var(--ol-motion-quick), transform 0.12s var(--ol-motion-quick)',
            }}
          >
            {t('shell.hotkeyModePrompt.openSettings')}
          </button>
        </div>
      </div>
    </div>
  );
}
