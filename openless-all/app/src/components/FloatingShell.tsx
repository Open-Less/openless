// FloatingShell.tsx — frosted outer frame + raised inner console.
// Sidebar lives INSIDE the console card. Footer icons sit on the frosted outer.
// Settings is no longer a sidebar tab — it opens as a centered modal sheet.
//
// Ported verbatim from design_handoff_openless/variants.jsx::FloatingShell.

import { useEffect, useMemo, useState, type CSSProperties, type ComponentType, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { isDialogStatus, UpdateDialog, useAutoUpdate } from './AutoUpdate';
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
import { formatComboLabel } from '../lib/hotkey';
import { applyFontScale, readFontScale } from '../lib/fontScale';
import { getCredentials, openExternal } from '../lib/ipc';
import {
  PROVIDER_SETUP_PROMPT_DEFERRED_KEY,
  shouldShowProviderSetupPrompt,
} from '../lib/providerSetup';
import { NAVIGATE_LOCAL_ASR_EVENT, type SettingsSectionId } from '../pages/Settings';
import { useHotkeySettings } from '../state/HotkeySettingsContext';
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

const RELEASE_NOTES_URL = 'https://github.com/appergb/openless/releases';
const HELP_DOCS_URL = 'https://github.com/appergb/openless#readme';

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
  const [helpPopoverOpen, setHelpPopoverOpen] = useState(false);
  const { prefs } = useHotkeySettings();

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

  // help popover 打开时，点击其他位置自动关闭
  useEffect(() => {
    if (!helpPopoverOpen) return;
    const onDown = (e: MouseEvent) => {
      const target = e.target as Element | null;
      if (target && target.closest('[data-ol-footer-popover]')) return;
      setHelpPopoverOpen(false);
    };
    const id = window.setTimeout(() => document.addEventListener('mousedown', onDown), 0);
    return () => {
      window.clearTimeout(id);
      document.removeEventListener('mousedown', onDown);
    };
  }, [helpPopoverOpen]);
  const NAV = useMemo<NavItem[]>(
    () => NAV_BASE.map(b => ({ ...b, name: t(`nav.${b.id}`) })),
    [t],
  );
  const Page = (NAV.find((n) => n.id === displayTab) ?? NAV[0]).cmp;

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

          {/* nav */}
          <nav style={{ display: 'flex', flexDirection: 'column', gap: 1 }}>
            {NAV.map((n) => {
              const active = currentTab === n.id;
              return (
                <button
                  key={n.id}
                  onClick={() => setCurrentTab(n.id)}
                  style={{
                    display: 'flex', alignItems: 'center', gap: 10,
                    padding: '7px 10px',
                    borderRadius: 8, border: 0,
                    background: active ? 'var(--ol-surface)' : 'transparent',
                    color: active ? 'var(--ol-ink)' : 'var(--ol-ink-3)',
                    fontFamily: 'inherit', fontSize: 13, fontWeight: active ? 600 : 500,
                    boxShadow: active ? '0 1px 2px rgba(0,0,0,.05), 0 0 0 0.5px rgba(0,0,0,.06)' : 'none',
                    cursor: 'default',
                    transition: 'background 0.16s var(--ol-motion-quick), color 0.16s var(--ol-motion-quick), box-shadow 0.18s var(--ol-motion-soft)',
                    textAlign: 'left',
                  }}>

                  <Icon name={n.icon} size={14} />
                  <span style={{ flex: 1 }}>{n.name}</span>
                </button>
              );
            })}
          </nav>

          <div style={{ flex: 1 }} />

          {/* shortcut hint — 不要 dashed 边框，否则会切断"整片磨砂玻璃"的视觉 */}
          <div style={{ padding: '10px 10px 6px', marginTop: 6 }}>
            <div style={{ fontSize: 10.5, color: 'var(--ol-ink-4)', marginBottom: 6, letterSpacing: '0.02em' }}>{t('shell.shortcutLabel')}</div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 11, color: 'var(--ol-ink-2)' }}>
              <kbd style={{
              padding: '2px 7px', fontSize: 10.5,
                background: 'rgba(255,255,255,0.7)', borderRadius: 5,
                border: '0.5px solid var(--ol-line-strong)',
                fontFamily: 'var(--ol-font-mono)', color: 'var(--ol-ink)',
                boxShadow: '0 1px 0 rgba(0,0,0,.04)',
              }}>{prefs ? formatComboLabel(prefs.dictationHotkey) : ''}</kbd>
              <span style={{ color: 'var(--ol-ink-4)' }}>{t('shell.shortcutHint')}</span>
            </div>
          </div>

          {/* BETA 区域 — 仅在 Beta build（version 含 semver prerelease 段，如 1.2.24-1）显示。
              正式版 build 完全不渲染这块，避免普通用户误以为正式版是 Beta。 */}
          {IS_BETA_BUILD && (
            <div style={{ marginTop: 8, padding: '10px 10px 4px' }}>
              <div style={{ fontSize: 10.5, fontWeight: 600, color: 'var(--ol-blue)', letterSpacing: '0.04em', textTransform: 'uppercase' }}>{t('shell.betaTag')}</div>
              <div style={{ fontSize: 11.5, color: 'var(--ol-ink-2)', marginTop: 4, lineHeight: 1.5 }}>{t('shell.betaNote')}</div>
            </div>
          )}
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

        <FooterIcon name="user" tip={t('shell.footer.account')} onClick={() => openSettings('providers')} />
        <FooterIcon name="mail" tip={t('shell.footer.feedback')} onClick={() => openExternal('https://github.com/appergb/openless/issues')} />
        <FooterIcon name="settings" tip={t('shell.footer.settings')} active={settingsOpen} onClick={() => openSettings()} />

        {/* 问号 — 点击展开版本说明 popover */}
        <FooterIconWithPopover
          name="help"
          tip={t('shell.footer.help')}
          open={helpPopoverOpen}
          onToggle={() => setHelpPopoverOpen(o => !o)}
        >
          <HelpPopoverBody />
        </FooterIconWithPopover>

        <div style={{ flex: 1 }} />

        <span style={{ fontFamily: 'var(--ol-font-sans)' }}>{t('shell.footer.version', { version: APP_VERSION_LABEL })}</span>
        <FooterAutoUpdateButton />
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
        @keyframes ol-popover-pop {
          from { opacity: 0; transform: translateY(6px) scale(.96); filter: blur(6px); }
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

// 把 footer icon 和它的 popover 绑在同一个相对定位容器里，popover 锚定在按钮正上方。
function FooterIconWithPopover({
  name, tip, open, onToggle, children,
}: {
  name: string;
  tip: string;
  open: boolean;
  onToggle: () => void;
  children: ReactNode;
}) {
  return (
    <div data-ol-footer-popover style={{ position: 'relative', display: 'inline-flex' }}>
      <FooterIcon name={name} tip={tip} active={open} onClick={onToggle} />
      {open && (
        <div
          style={{
            position: 'absolute',
            bottom: 'calc(100% + 8px)',
            left: 0,
            zIndex: 80,
            minWidth: 220,
            padding: 12,
            borderRadius: 12,
            background: 'rgba(255,255,255,0.96)',
            backdropFilter: 'blur(var(--ol-glass-blur)) saturate(180%)',
            WebkitBackdropFilter: 'blur(var(--ol-glass-blur)) saturate(180%)',
            border: '0.5px solid rgba(0,0,0,0.08)',
            boxShadow: '0 18px 50px -22px rgba(15,17,22,0.32), 0 0 0 0.5px rgba(0,0,0,0.05)',
            animation: 'ol-popover-pop 0.22s var(--ol-motion-spring) both',
            transformOrigin: 'bottom left',
          }}
        >
          {children}
        </div>
      )}
    </div>
  );
}

function HelpPopoverBody() {
  const { t } = useTranslation();
  return (
    <div style={{ minWidth: 240 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 8 }}>
        <img src="AppIcon.png" alt="" style={{ width: 26, height: 26, borderRadius: 6, boxShadow: '0 1px 2px rgba(0,0,0,.1), 0 0 0 0.5px rgba(0,0,0,.06)' }} />
        <div>
          <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--ol-ink)' }}>OpenLess</div>
          <div style={{ fontSize: 11, color: 'var(--ol-ink-4)', fontFamily: 'var(--ol-font-mono)', marginTop: 1 }}>{APP_VERSION_LABEL}</div>
        </div>
      </div>
      <div style={{ fontSize: 11.5, color: 'var(--ol-ink-3)', lineHeight: 1.55, marginBottom: 10 }}>
        {t('shell.footer.helpPopover.tagline')}
      </div>
      <button onClick={() => openExternal(RELEASE_NOTES_URL)} style={popoverLinkStyle}>
        {t('shell.footer.helpPopover.releaseNotes')}
      </button>
      <button onClick={() => openExternal(HELP_DOCS_URL)} style={popoverLinkStyle}>
        {t('shell.footer.helpPopover.docs')}
      </button>
    </div>
  );
}

const popoverLinkStyle: CSSProperties = {
  display: 'block',
  width: '100%',
  border: 0,
  background: 'transparent',
  color: 'var(--ol-blue)',
  fontFamily: 'inherit',
  fontSize: 12,
  fontWeight: 500,
  cursor: 'default',
  textAlign: 'left',
  padding: '6px 4px',
};

// Footer 的"检查更新"按钮 — 复用 Settings 页面的 useAutoUpdate hook + UpdateDialog 窗口，
// 跟"关于"section 走完全相同的状态机和确认对话框。按钮本身只显示触发文案 + 简短状态。
function FooterAutoUpdateButton() {
  const { t } = useTranslation();
  const u = useAutoUpdate();

  const inlineHint = u.status === 'none'
    ? t('settings.about.upToDate')
    : u.status === 'error'
      ? t('settings.about.updateError')
      : null;
  const inlineHintColor = u.status === 'error' ? 'var(--ol-err)' : 'var(--ol-ink-4)';

  return (
    <>
      <button
        onClick={u.checkForUpdates}
        disabled={u.checking || u.busy}
        style={{
          color: 'var(--ol-blue)',
          marginLeft: 8,
          textDecoration: 'none',
          fontWeight: 500,
          border: 0,
          background: 'transparent',
          fontFamily: 'inherit',
          fontSize: 11,
          cursor: 'default',
          padding: 0,
          opacity: u.checking || u.busy ? 0.7 : 1,
          transition: 'opacity 0.16s var(--ol-motion-soft)',
        }}
      >
        {u.checking ? t('settings.about.checkingUpdate') : t('settings.about.checkUpdateBtn')}
      </button>
      {inlineHint && (
        <span style={{ marginLeft: 8, color: inlineHintColor, fontSize: 11 }}>{inlineHint}</span>
      )}
      {isDialogStatus(u.status) && (
        <UpdateDialog
          status={u.status}
          version={u.version}
          progress={u.progress}
          downloaded={u.downloaded}
          contentLength={u.contentLength}
          onInstall={u.installUpdate}
          onClose={u.dismissDialog}
        />
      )}
    </>
  );
}
