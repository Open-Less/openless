// FloatingShell.tsx — frosted outer frame + raised inner console.
// Sidebar lives INSIDE the console card. Footer icons sit on the frosted outer.
// Settings is no longer a sidebar tab — it opens as a centered modal sheet.
//
// Ported verbatim from design_handoff_openless/variants.jsx::FloatingShell.

import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ComponentType } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from './Icon';
import { WindowChrome, detectOS, type OS } from './WindowChrome';
import { SettingsModal } from './SettingsModal';
import { Overview } from '../pages/Overview';
import { History } from '../pages/History';
import { Vocab } from '../pages/Vocab';
import { Style } from '../pages/Style';
import { Translation } from '../pages/Translation';
import { SelectionAsk } from '../pages/SelectionAsk';
import { LocalAsr } from '../pages/LocalAsr';
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
import { NAVIGATE_LOCAL_ASR_EVENT, type SettingsSectionId } from '../pages/Settings';
import { useAppState, type AppTab } from '../state/useAppState';

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
  { id: 'localAsr', icon: 'archive', cmp: LocalAsr },
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
    const idx = NAV.findIndex(n => n.id === currentTab);
    const el = navItemRefs.current[idx];
    if (!el) return;
    setPillRect({ top: el.offsetTop, height: el.offsetHeight });
  }, [currentTab, NAV]);

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

  // Settings → ASR 选 local-qwen3 时的"前往模型设置"事件 → 关 modal + 切 tab。
  useEffect(() => {
    const onNavigate = () => {
      setSettingsOpen(false);
      setCurrentTab('localAsr');
    };
    window.addEventListener(NAVIGATE_LOCAL_ASR_EVENT, onNavigate);
    return () => window.removeEventListener(NAVIGATE_LOCAL_ASR_EVENT, onNavigate);
  }, [setCurrentTab, setSettingsOpen]);

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
    openSettings('providers');
  };

  const openHotkeyRecordingSettings = () => {
    window.localStorage.setItem(HOTKEY_MODE_MIGRATION_ACK_KEY, '1');
    setHotkeyModePromptOpen(false);
    openSettings('recording');
  };

  return (
    <div style={{ flex: 1, position: 'relative', display: 'flex', flexDirection: 'column', minHeight: 0, paddingTop: os === 'mac' ? 28 : 0 }}>

      {/* Main shell — flush with the frosted backplate (no separate float). */}
      <div
        style={{
          flex: 1, minHeight: 0,
          display: 'flex',
          background: 'transparent',
          overflow: 'hidden',
          position: 'relative',
          zIndex: 1,
        }}>

        {/* Sidebar — 透明地坐在外层磨砂底板上，让 LOGO/导航/快捷键/BETA/footer 共用同一片磨砂玻璃 */}
        <aside
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
              const active = currentTab === n.id;
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
        </aside>

        {/* Main content — inset white card sitting on the frosted backplate.
            内卡圆角与外层窗口（WindowChrome 20/14）对齐，避免视觉上"两个不一致的圆角"。 */}
        <div style={{ flex: 1, minWidth: 0, padding: '4px 8px 6px 0', display: 'flex' }}>
          <main
            style={{
              flex: 1, minWidth: 0,
              overflow: 'hidden',
              background: 'var(--ol-surface)',
              borderRadius: 'var(--ol-window-console-radius)',
              border: '0.5px solid rgba(0,0,0,0.06)',
              boxShadow: '0 1px 0 rgba(255,255,255,0.8) inset, 0 8px 24px -12px rgba(15,17,22,0.10), 0 2px 6px -2px rgba(15,17,22,0.06)',
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
                padding: '24px 28px 32px',
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
      </div>

      {/* Footer — 透明地坐在外层磨砂底板上，跟 sidebar 同一片磨砂玻璃 */}
      <div
        style={{
          flexShrink: 0,
          height: 44,
          display: 'flex', alignItems: 'center',
          padding: '0 24px',
          gap: 4,
          fontSize: 11,
          color: 'var(--ol-ink-4)',
          position: 'relative',
          zIndex: 2,
        }}>

        <div style={{ flex: 1 }} />

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
            marginRight: 8,
          }}>{t('shell.betaTag')}</span>
        )}

        <span style={{ fontFamily: 'var(--ol-font-sans)', marginRight: 12 }}>{t('shell.footer.version', { version: APP_VERSION_LABEL })}</span>
        <div style={{ marginRight: 12 }}>
          <FooterIcon name="settings" tip={t('shell.footer.settings')} active={settingsOpen} onClick={() => openSettings()} />
        </div>
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
          background: rgba(0,0,0,0.04);
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
              background: 'var(--ol-ink)',
              color: '#fff',
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
              background: 'var(--ol-ink)',
              color: '#fff',
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

interface FooterIconProps {
  name: string;
  tip: string;
  active?: boolean;
  onClick?: () => void;
}

function FooterIcon({ name, tip, active, onClick }: FooterIconProps) {
  const [hover, setHover] = useState(false);
  // 选中（active）= popover 打开，深灰；hover = 浅灰；其它 = 透明
  const background = active
    ? 'rgba(0,0,0,0.10)'
    : hover
      ? 'rgba(0,0,0,0.05)'
      : 'transparent';
  return (
    <button
      onClick={onClick}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      title={tip}
      style={{
        width: 30, height: 30, borderRadius: 7, border: 0,
        background,
        color: active ? 'var(--ol-ink)' : hover ? 'var(--ol-ink-2)' : 'var(--ol-ink-4)',
        display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
        cursor: 'default',
        transition: 'background 0.16s var(--ol-motion-quick), color 0.16s var(--ol-motion-quick)',
      }}>
      <Icon name={name} size={15} />
    </button>
  );
}

