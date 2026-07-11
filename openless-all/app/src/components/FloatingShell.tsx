// FloatingShell.tsx — frosted outer frame + raised inner console.
// Sidebar lives INSIDE the console card.
// Settings opens as a centered modal sheet from the sidebar bottom entry.
//
// Ported verbatim from design_handoff_openless/variants.jsx::FloatingShell.

import { useEffect, useMemo, useState, type ComponentType, type CSSProperties } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from './Icon';
import { Tooltip } from './Tooltip';
import { WindowChrome, detectOS, type OS } from './WindowChrome';
import { AudioCueListener } from "./AudioCue";
import { SettingsModal } from './SettingsModal';
import { Overview } from '../pages/Overview';
import { History } from '../pages/History';
import { Vocab } from '../pages/Vocab';
import { Style } from '../pages/Style';
import { Marketplace } from '../pages/Marketplace';
import { Translation } from '../pages/Translation';
import { SelectionAsk } from '../pages/SelectionAsk';
// 风格市场（Marketplace）现在是侧栏「风格」展开组下的独立页面（不再是 Style 页面内 modal）。
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

/** macOS 上侧栏顶部需让开原生红绿灯的高度（红绿灯竖直落在 ~6–22px）。 */
const MAC_TRAFFIC_LIGHT_CLEARANCE = 30;
const SIDEBAR_WIDTH = 188;

/** tab → 页面组件映射（渲染主内容用；与侧栏树解耦，含未直接列在树上的页）。 */
const PAGE_CMP: Record<Exclude<AppTab, 'localAsr'>, ComponentType> = {
  overview: Overview,
  history: History,
  vocab: Vocab,
  style: Style,
  marketplace: Marketplace,
  translation: Translation,
  selectionAsk: SelectionAsk,
};

/** 侧栏导航树：扁平项 + 可展开分组（用户拍板的结构）。
 *  - 概览 / 历史 / 词汇：扁平项，位置不变。
 *  - 风格：展开组 → 润色模式(=style 页) + 风格市场(=marketplace 页，原为 Style 内 modal)。
 *  - 工具：展开组 → 划词追问 + 翻译（原本两个扁平项合并成一个可展开分组）。 */
type NavNode =
  | { kind: 'item'; id: AppTab; icon: string }
  | { kind: 'group'; key: string; icon: string; children: Array<{ id: AppTab }> };

const NAV_TREE: NavNode[] = [
  { kind: 'item', id: 'overview', icon: 'overview' },
  { kind: 'item', id: 'history', icon: 'history' },
  { kind: 'item', id: 'vocab', icon: 'vocab' },
  { kind: 'group', key: 'style', icon: 'style', children: [{ id: 'style' }, { id: 'marketplace' }] },
  { kind: 'group', key: 'tools', icon: 'selectionAsk', children: [{ id: 'selectionAsk' }, { id: 'translation' }] },
];

/** 分组标题的 i18n key（nav.group.<key>）。子项标题：style 组用 nav.polishMode/nav.marketplace，
 *  其余子项复用 nav.<id>。 */
function subItemLabelKey(id: AppTab): string {
  if (id === 'style') return 'nav.polishMode';
  return `nav.${id}`;
}

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

  const Page = PAGE_CMP[displayTab as Exclude<AppTab, 'localAsr'>] ?? Overview;

  // 分组展开态：默认展开「当前所在页所属的分组」。用户点分组标题手动切换。
  const groupOfTab = (tab: AppTab): string | null => {
    for (const node of NAV_TREE) {
      if (node.kind === 'group' && node.children.some(c => c.id === tab)) return node.key;
    }
    return null;
  };
  const [openGroups, setOpenGroups] = useState<Record<string, boolean>>(() => {
    const active = groupOfTab(currentTab);
    return active ? { [active]: true } : {};
  });
  // 切到某分组内的页时自动展开该组（例如从胶囊直接跳到 marketplace）。
  useEffect(() => {
    const active = groupOfTab(currentTab);
    if (active) setOpenGroups(prev => (prev[active] ? prev : { ...prev, [active]: true }));
  }, [currentTab]);
  const toggleGroup = (key: string) => setOpenGroups(prev => ({ ...prev, [key]: !prev[key] }));

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
    : t(subItemLabelKey(currentTab));
  const moreTabActive = MORE_TAB_IDS.includes(currentTab);

  return (
    // 不再为 macOS 红绿灯预留顶部 28px 空条（用户反馈「块上方多一条丑横条」）：
    // 侧栏与内容块都顶到窗口最上沿，原生红绿灯直接浮在侧栏左上角的块面上。
    // 侧栏内 brand 行在 mac 上加 topClearance 让「OpenLess」避开红绿灯。
    <div style={{ flex: 1, position: 'relative', display: 'flex', flexDirection: 'column', minHeight: 0, paddingTop: 0, background: 'var(--ol-app-shell-bg)' }}>

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

        {/* Sidebar — desktop / wide only。 */}
        {!mobile && (
        <aside
          className="ol-sidebar-surface"
          style={{
            width: SIDEBAR_WIDTH,
            height: '100%',
            flexShrink: 0,
            display: 'flex', flexDirection: 'column',
            background: 'var(--ol-sidebar-bg)',
            borderRight: '0.5px solid var(--ol-line)',
            // mac：顶部空出红绿灯高度，让 brand/nav 落在红绿灯下方。
            padding: os === 'mac' ? `${MAC_TRAFFIC_LIGHT_CLEARANCE}px 10px 12px` : '10px 10px 12px',
          }}>

          {/* brand */}
          <div style={{ display: 'flex', alignItems: 'center', gap: 9, padding: '2px 8px 12px' }}>
            <img
              src="AppIcon.png"
              alt="OpenLess"
              style={{ width: 22, height: 22, borderRadius: 5, flexShrink: 0, boxShadow: '0 1px 2px rgba(0,0,0,.1), 0 0 0 0.5px rgba(0,0,0,.06)' }} />

            <span style={{ flex: 1, fontSize: 13.5, fontWeight: 600, letterSpacing: '-0.01em', color: 'var(--ol-ink)', whiteSpace: 'nowrap' }}>OpenLess</span>
          </div>

          {/* nav — 扁平项 + 可展开分组（用户拍板结构）。扁平项：概览/历史/词汇。
              分组：风格(润色模式+风格市场) / 工具(划词追问+翻译)。active 项由
              .ol-nav-btn-active class 给 surface-2 静态圆角底；分组标题点击展开/收起。 */}
          <nav style={{ display: 'flex', flexDirection: 'column', gap: 1 }}>
            {NAV_TREE.map((node) => {
              if (node.kind === 'item') {
                const active = !settingsOpen && currentTab === node.id;
                return (
                  <Tooltip key={node.id} content={t(`shell.navHint.${node.id}`)} placement="right">
                    <button
                      onClick={() => setCurrentTab(node.id)}
                      className={active ? 'ol-nav-btn ol-nav-btn-active' : 'ol-nav-btn'}
                      style={navBtnStyle}
                    >
                      <Icon name={node.icon} size={14} />
                      <span style={{ flex: 1 }}>{t(`nav.${node.id}`)}</span>
                    </button>
                  </Tooltip>
                );
              }
              const expanded = !!openGroups[node.key];
              const groupActive = !settingsOpen && node.children.some(c => c.id === currentTab);
              return (
                <div key={node.key}>
                  {/* 分组标题：点击只展开/收起，不导航（用户：点风格弹出下拉选项）。 */}
                  <button
                    onClick={() => toggleGroup(node.key)}
                    className={groupActive && !expanded ? 'ol-nav-btn ol-nav-btn-active' : 'ol-nav-btn'}
                    aria-expanded={expanded}
                    style={navBtnStyle}
                  >
                    <Icon name={node.icon} size={14} />
                    <span style={{ flex: 1 }}>{t(`nav.group.${node.key}`)}</span>
                    <svg
                      width="12" height="12" viewBox="0 0 16 16" fill="none" aria-hidden
                      style={{
                        color: 'var(--ol-ink-4)', flexShrink: 0,
                        transform: expanded ? 'rotate(90deg)' : 'rotate(0deg)',
                        transition: 'transform 0.20s var(--ol-motion-spring)',
                      }}
                    >
                      <path d="M6 3.5 10.5 8 6 12.5" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" />
                    </svg>
                  </button>
                  {/* 子项：grid-rows 0fr↔1fr 过渡实现无高度硬编码的展开动画。 */}
                  <div
                    style={{
                      display: 'grid',
                      gridTemplateRows: expanded ? '1fr' : '0fr',
                      transition: 'grid-template-rows 0.24s var(--ol-motion-spring)',
                    }}
                  >
                    <div style={{ overflow: 'hidden', minHeight: 0 }}>
                      <div style={{ display: 'flex', flexDirection: 'column', gap: 1, padding: '1px 0 2px' }}>
                        {node.children.map(child => {
                          const active = !settingsOpen && currentTab === child.id;
                          return (
                            <button
                              key={child.id}
                              onClick={() => setCurrentTab(child.id)}
                              className={active ? 'ol-nav-btn ol-nav-subitem ol-nav-btn-active' : 'ol-nav-btn ol-nav-subitem'}
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
                  background: 'var(--ol-blue-soft)',
                  border: '0.5px solid var(--ol-pill-blue-border)',
                  borderRadius: 999,
                }}>{t('shell.betaTag')}</span>
              )}

              <span>{t('shell.footer.version', { version: APP_VERSION_LABEL })}</span>
            </div>

            <Tooltip content={t('shell.navHint.settings')} placement="right">
              <button
                onClick={() => openSettings()}
                className={settingsOpen ? 'ol-nav-btn ol-nav-btn-active' : 'ol-nav-btn'}
                style={{
                  display: 'flex', alignItems: 'center', gap: 10,
                  padding: '7px 10px',
                  borderRadius: 8, border: 0,
                  fontFamily: 'inherit', fontSize: 13,
                  cursor: 'default',
                  transition: 'color 0.16s var(--ol-motion-quick), background 0.16s var(--ol-motion-quick)',
                  textAlign: 'left',
                }}
              >
                <Icon name="settings" size={14} />
                <span style={{ flex: 1 }}>{t('shell.footer.settings')}</span>
              </button>
            </Tooltip>
          </div>
        </aside>
        )}

        {/* Main content — 平铺实底块（用户反馈：右侧要一整块、不要圆角、别浮成卡）。
            surface 实底，无外边距无圆角无阴影，直接顶到窗口边缘，与侧栏之间仅靠
            侧栏的 border-right 分隔（即「侧栏那套逻辑」）。mac 圆角由原生窗口裁切。 */}
        <div style={{ flex: 1, minWidth: 0, padding: 0, display: 'flex' }}>
          <main
            className="ol-console-main"
            style={{
              flex: 1, minWidth: 0,
              overflow: 'hidden',
              background: 'var(--ol-surface)',
              borderRadius: 0,
              border: 'none',
              boxShadow: 'none',
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
              className="ol-thinscroll ol-scroll-fade"
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
                willChange: mobile ? 'opacity' : 'opacity, transform',
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
        /* nav 三段视觉层次（扁平静态，无 pill 滑块）：
             基础态  → ink-3（中灰文字 + 透明底）
             hover  → ink（深色文字 + surface-2 浅灰底）  ← 让"翻译"等字词在悬停时高亮
             选中  → ink（深色文字加粗 + surface-2 静态圆角底）
           全走 class：sidebar 按钮不写 inline background，:hover / active 底色才能生效
           （CSS 不能盖 inline style）；其他 .ol-nav-btn 使用方仍带 inline background，
           这里的 active 底色对它们不生效，维持各自原样。 */
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
        /* 只动 opacity/transform（合成器友好）：filter:blur 每帧全页重算，
           是 tab 切换「卡」的元凶，已移除。 */
        @keyframes ol-page-slide {
          from { opacity: 0; transform: translate3d(10px, 0, 0) scale(.996); }
          to   { opacity: 1; transform: translate3d(0, 0, 0) scale(1); }
        }
        @keyframes ol-page-fadeout {
          from { opacity: 1; transform: translate3d(0, 0, 0); }
          to   { opacity: 0; transform: translate3d(-6px, 0, 0); }
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

/** 侧栏导航按钮基础样式（扁平项 / 分组标题 / 子项共用；子项额外覆盖 paddingLeft）。 */
const navBtnStyle: CSSProperties = {
  display: 'flex', alignItems: 'center', gap: 10,
  width: '100%',
  padding: '7px 10px',
  borderRadius: 8, border: 0,
  fontFamily: 'inherit', fontSize: 13,
  cursor: 'default',
  transition: 'color 0.16s var(--ol-motion-quick), background 0.16s var(--ol-motion-quick)',
  textAlign: 'left',
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
