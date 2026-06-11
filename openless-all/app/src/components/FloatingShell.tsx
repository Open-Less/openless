// FloatingShell.tsx — frosted outer frame + raised inner console.
// Sidebar lives INSIDE the console card.
// Settings opens as a centered modal sheet from the sidebar bottom entry.
//
// Ported verbatim from design_handoff_openless/variants.jsx::FloatingShell.

import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ComponentType } from 'react';
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
import { getCredentials, getPlatformCapabilities } from '../lib/ipc';
import {
  PROVIDER_SETUP_PROMPT_DEFERRED_KEY,
  shouldShowProviderSetupPrompt,
} from '../lib/providerSetup';
import { type SettingsSectionId } from './SettingsModal';
import { useAppState, type AppTab } from '../state/useAppState';
import { useMobileLayout } from '../lib/useMobileLayout';

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
  const { currentTab, setCurrentTab, settingsOpen, setSettingsOpen } = useAppState(initialTab, initialSettings);
  const [settingsInitialSection, setSettingsInitialSection] = useState<SettingsSectionId | undefined>();
  const [providerPromptOpen, setProviderPromptOpen] = useState(false);
  const [hotkeyModePromptOpen, setHotkeyModePromptOpen] = useState(false);
  const mobileLayout = useMobileLayout();

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
  const activeNav = NAV.find(n => n.id === currentTab) ?? NAV[0];

  // sidebar nav 滑动指示器：测量当前 active button 的 offsetTop / height，
  // 用一个 absolute pill 平滑滑过去，而不是每个按钮各自瞬切背景色。
  const navItemRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const [pillRect, setPillRect] = useState<{ top: number; height: number } | null>(null);
  useLayoutEffect(() => {
    if (mobileLayout) {
      setPillRect(null);
      return;
    }
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
  }, [currentTab, settingsOpen, NAV, mobileLayout]);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      const caps = await getPlatformCapabilities();
      if (cancelled || caps.platform === 'android') return;
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
    let cancelled = false;
    void getPlatformCapabilities().then((caps) => {
      if (cancelled || caps.platform === 'android') return;
      const acknowledgedValue = window.localStorage.getItem(HOTKEY_MODE_MIGRATION_ACK_KEY);
      const deferredValue = window.sessionStorage.getItem(HOTKEY_MODE_MIGRATION_DEFERRED_KEY);
      if (shouldShowHotkeyModeMigrationPrompt(acknowledgedValue, deferredValue)) {
        setHotkeyModePromptOpen(true);
      }
    });
    return () => {
      cancelled = true;
    };
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

  return (
    <div
      className="ol-app-shell-bg"
      style={{ flex: 1, position: 'relative', display: 'flex', flexDirection: 'column', minHeight: 0, paddingTop: os === 'mac' ? 28 : 0 }}
    >

      {/* Main shell — flush with the frosted backplate (no separate float). */}
      <div
        style={{
          flex: 1, minHeight: 0,
          display: 'flex',
          flexDirection: mobileLayout ? 'column' : 'row',
          background: 'transparent',
          overflow: 'hidden',
          position: 'relative',
          zIndex: 1,
        }}>

        {mobileLayout ? (
          <div className="ol-aura-mobile-topbar">
            <div className="ol-aura-mobile-brand">
              <img
                className="ol-aura-mobile-brand-mark"
                src="AppIcon.png"
                alt="OpenLess"
              />
              <div style={{ minWidth: 0 }}>
                <div className="ol-aura-mobile-brand-title">OpenLess</div>
                <div className="ol-aura-mobile-brand-section">{activeNav.name}</div>
              </div>
            </div>
            <button
              type="button"
              onClick={() => openSettings()}
              className={settingsOpen ? 'ol-aura-mobile-settings ol-aura-mobile-settings-active' : 'ol-aura-mobile-settings'}
              aria-label={t('shell.footer.settings')}
            >
              <Icon name="settings" size={17} />
            </button>
          </div>
        ) : (
          /* Sidebar — 透明地坐在外层磨砂底板上，让 LOGO/导航/快捷键/BETA/footer 共用同一片磨砂玻璃 */
          <aside
            className="ol-aura-sidebar"
            style={{
              width: 196,
              flexShrink: 0,
              display: 'flex',
              flexDirection: 'column',
            }}>

          {/* brand */}
          <div className="ol-aura-sidebar-brand">
            <img
              className="ol-aura-sidebar-brand-mark"
              src="AppIcon.png"
              alt="OpenLess"
            />

            <div>
              <div className="ol-aura-sidebar-brand-title">OpenLess</div>
              <div className="ol-aura-sidebar-brand-kicker">VOICE CONSOLE</div>
            </div>
          </div>

          {/* nav — 滑动指示器：active pill 是 absolute 元素，currentTab 改变时 top/height
              过渡到目标按钮的位置，而非各按钮自己瞬切背景色。hover 灰底通过 .ol-nav-btn 的
              CSS :hover 规则实现，仅对非 active 项生效。 */}
          <nav style={{ position: 'relative', display: 'flex', flexDirection: 'column', gap: 1 }}>
            {pillRect && (
              <div
                className="ol-aura-sidebar-pill"
                aria-hidden
                style={{
                  position: 'absolute',
                  left: 0,
                  right: 0,
                  top: pillRect.top,
                  height: pillRect.height,
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
                  className={active ? 'ol-nav-btn ol-nav-btn-active ol-aura-sidebar-nav-btn' : 'ol-nav-btn ol-aura-sidebar-nav-btn'}
                  style={{
                    display: 'flex', alignItems: 'center', gap: 10,
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
          <div className="ol-aura-sidebar-footer">
            <div className="ol-aura-sidebar-version">
              {IS_BETA_BUILD && (
                <span className="ol-aura-beta-tag">{t('shell.betaTag')}</span>
              )}

              <span>{t('shell.footer.version', { version: APP_VERSION_LABEL })}</span>
            </div>

            <button
              onClick={() => openSettings()}
              className={settingsOpen ? 'ol-nav-btn ol-nav-btn-active ol-aura-sidebar-settings' : 'ol-nav-btn ol-aura-sidebar-settings'}
              style={{
                display: 'flex', alignItems: 'center', gap: 10,
              }}
            >
              <Icon name="settings" size={14} />
              <span style={{ flex: 1 }}>{t('shell.footer.settings')}</span>
            </button>
          </div>
          </aside>
        )}

        {/* Main content — Linux 禁用透明窗口后使用不透明面；其他平台保留玻璃层。 */}
        <div
          style={{
            flex: 1,
            minWidth: 0,
            minHeight: 0,
            padding: mobileLayout ? '0' : '4px 8px 6px 0',
            display: 'flex',
          }}
        >
          <main
            className="ol-console-main ol-aura-panel ol-aura-console-main"
            style={{
              flex: 1, minWidth: 0,
              overflow: 'hidden',
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
                padding: mobileLayout
                  ? '16px 14px 18px'
                  : '24px 28px 32px',
                // position:relative 让页面里的"已保存"toast 用 absolute top:16 right:16
                // 锚到这块控制台卡的右上角，而不是横在页头变成长横幅。
                position: 'relative',
                // 苹果"spring out"风格的曲线：开始快、收尾顺滑，符合人体直觉
                animation: tabPhase === 'exiting'
                  ? 'ol-page-fadeout 0.18s var(--ol-motion-soft) forwards'
                  : 'ol-page-slide 0.34s var(--ol-motion-spring) both',
                willChange: 'opacity, transform, filter',
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

        {mobileLayout && (
          <nav className="ol-aura-mobile-nav" aria-label="OpenLess">
            {NAV.map(n => {
              const active = !settingsOpen && currentTab === n.id;
              return (
                <button
                  key={n.id}
                  type="button"
                  onClick={() => setCurrentTab(n.id)}
                  className={active ? 'ol-aura-mobile-nav-btn ol-aura-mobile-nav-btn-active' : 'ol-aura-mobile-nav-btn'}
                >
                  <Icon name={n.icon} size={18} />
                  <span>{n.name}</span>
                </button>
              );
            })}
          </nav>
        )}
      </div>

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
        .ol-aura-sidebar {
          padding: 14px 12px 14px;
          background: var(--ol-sidebar-bg);
          border-right: 1px solid var(--ol-sidebar-border);
        }
        .ol-aura-sidebar-brand {
          display: flex;
          align-items: center;
          gap: 10px;
          padding: 6px 10px 16px;
          margin-bottom: 6px;
          border-radius: 0;
          background: var(--ol-sidebar-brand-bg);
          border: 1px solid var(--ol-sidebar-brand-border);
          box-shadow: none;
        }
        .ol-aura-sidebar-brand-mark {
          width: 26px;
          height: 26px;
          border-radius: 8px;
          box-shadow: none;
          box-sizing: border-box;
          padding: 3px;
          object-fit: contain;
        }
        .ol-aura-sidebar-brand-title {
          font-size: 14px;
          font-weight: 600;
          font-family: var(--ol-font-display);
          color: var(--ol-ink);
        }
        .ol-aura-sidebar-brand-kicker {
          font-size: 10.5px;
          color: var(--ol-ink-4);
          font-family: var(--ol-font-mono);
          letter-spacing: .08em;
        }
        .ol-aura-sidebar-pill {
          background: var(--ol-sidebar-pill-bg);
          border-radius: 12px;
          border: 1px solid var(--ol-sidebar-pill-border);
          box-shadow: none;
        }
        .ol-aura-sidebar-nav-btn {
          padding: 8px 12px;
          border-radius: 12px;
          border: 0;
          background: transparent;
          font-family: inherit;
          font-size: 13px;
          cursor: default;
          transition: color 0.16s var(--ol-motion-quick), background 0.16s var(--ol-motion-quick);
          text-align: left;
          position: relative;
          z-index: 1;
        }
        .ol-aura-sidebar-footer {
          display: flex;
          flex-direction: column;
          gap: 10px;
          padding: 12px 10px 0;
          margin-top: 10px;
          border-top: 1px solid var(--ol-sidebar-footer-border);
        }
        .ol-aura-sidebar-version {
          display: flex;
          align-items: center;
          gap: 8px;
          flex-wrap: wrap;
          padding: 10px 12px;
          font-family: var(--ol-font-sans);
          font-size: 11px;
          color: var(--ol-ink-4);
          background: var(--ol-sidebar-version-bg);
          border: 1px solid var(--ol-sidebar-version-border);
          border-radius: var(--ol-pill-radius);
          box-shadow: none;
        }
        .ol-aura-beta-tag {
          display: inline-block;
          padding: 2px 8px;
          font-size: 10px;
          font-weight: 600;
          letter-spacing: 0.04em;
          text-transform: uppercase;
          color: var(--ol-blue);
          background: rgba(37,99,235,0.10);
          border-radius: 999px;
        }
        .ol-aura-sidebar-settings {
          padding: 10px 12px;
          border-radius: 12px;
          border: 1px solid var(--ol-sidebar-settings-border);
          background: var(--ol-sidebar-settings-bg);
          box-shadow: none;
        }
        .ol-aura-sidebar-settings.ol-nav-btn-active {
          background: var(--ol-sidebar-settings-active-bg);
          box-shadow: none;
        }
        .ol-aura-console-main {
          border-radius: ${mobileLayout ? '0' : 'var(--ol-panel-radius)'};
        }
        .ol-aura-mobile-topbar {
          flex-shrink: 0;
          display: flex;
          align-items: center;
          justify-content: space-between;
          gap: 12px;
          padding: calc(10px + env(safe-area-inset-top, 0px)) 14px 10px;
          border-bottom: 1px solid var(--ol-sidebar-border);
          background: var(--ol-sidebar-bg);
        }
        .ol-aura-mobile-brand {
          min-width: 0;
          display: flex;
          align-items: center;
          gap: 10px;
        }
        .ol-aura-mobile-brand-mark {
          width: 30px;
          height: 30px;
          border-radius: 8px;
          flex-shrink: 0;
          box-sizing: border-box;
          padding: 3px;
          object-fit: contain;
        }
        .ol-aura-mobile-brand-title {
          font-size: 14px;
          font-weight: 700;
          color: var(--ol-ink);
          line-height: 1.15;
        }
        .ol-aura-mobile-brand-section {
          margin-top: 2px;
          font-size: 11px;
          color: var(--ol-ink-4);
          font-family: var(--ol-font-mono);
          white-space: nowrap;
          overflow: hidden;
          text-overflow: ellipsis;
        }
        .ol-aura-mobile-settings {
          width: 36px;
          height: 36px;
          flex-shrink: 0;
          display: inline-flex;
          align-items: center;
          justify-content: center;
          border-radius: 12px;
          color: var(--ol-ink-3);
          background: var(--ol-sidebar-settings-bg);
          border: 1px solid var(--ol-sidebar-settings-border);
        }
        .ol-aura-mobile-settings-active {
          color: var(--ol-ink);
          background: var(--ol-sidebar-settings-active-bg);
        }
        .ol-aura-mobile-nav {
          flex-shrink: 0;
          display: grid;
          grid-template-columns: repeat(${NAV.length}, minmax(0, 1fr));
          gap: 2px;
          padding: 7px 8px calc(7px + env(safe-area-inset-bottom, 0px));
          border-top: 1px solid var(--ol-sidebar-border);
          background: var(--ol-sidebar-bg);
        }
        .ol-aura-mobile-nav-btn {
          min-width: 0;
          height: 50px;
          display: inline-flex;
          flex-direction: column;
          align-items: center;
          justify-content: center;
          gap: 4px;
          border-radius: 12px;
          color: var(--ol-ink-4);
          font-size: 10px;
          font-weight: 600;
          line-height: 1.1;
        }
        .ol-aura-mobile-nav-btn span {
          max-width: 100%;
          overflow: hidden;
          white-space: nowrap;
          text-overflow: ellipsis;
        }
        .ol-aura-mobile-nav-btn-active {
          color: var(--ol-ink);
          background: var(--ol-sidebar-pill-bg);
          border: 1px solid var(--ol-sidebar-pill-border);
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
        background: 'var(--ol-overlay-bg)',
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
              background: 'var(--ol-ink)',
              color: 'var(--ol-on-accent)',
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
        background: 'var(--ol-overlay-bg)',
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
              background: 'var(--ol-ink)',
              color: 'var(--ol-on-accent)',
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
