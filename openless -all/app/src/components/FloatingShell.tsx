// FloatingShell.tsx — frosted outer frame + raised inner console.
// Sidebar lives INSIDE the console card. Footer icons sit on the frosted outer.
// Settings is no longer a sidebar tab — it opens as a centered modal sheet.
//
// Ported verbatim from design_handoff_openless/variants.jsx::FloatingShell.

import { useEffect, useState, type ComponentType } from 'react';
import { Icon } from './Icon';
import { WindowChrome, detectOS, type OS } from './WindowChrome';
import { SettingsModal } from './SettingsModal';
import { Overview } from '../pages/Overview';
import { History } from '../pages/History';
import { Vocab } from '../pages/Vocab';
import { Style } from '../pages/Style';
import { APP_VERSION_LABEL } from '../lib/appVersion';
import { getHotkeyTriggerLabel } from '../lib/hotkey';
import { getCredentials, openExternal } from '../lib/ipc';
import { OL_DATA } from '../lib/mockData';
import {
  PROVIDER_SETUP_PROMPT_SEEN_KEY,
  shouldShowProviderSetupPrompt,
} from '../lib/providerSetup';
import type { SettingsSectionId } from '../pages/Settings';
import { useHotkeySettings } from '../state/HotkeySettingsContext';
import { useAppState, type AppTab } from '../state/useAppState';

interface NavItem {
  id: AppTab;
  name: string;
  icon: string;
  cmp: ComponentType;
}

const NAV: NavItem[] = [
  { id: 'overview', name: '概览', icon: 'overview', cmp: Overview },
  { id: 'history', name: '历史', icon: 'history', cmp: History },
  { id: 'vocab', name: '词汇表', icon: 'vocab', cmp: Vocab },
  { id: 'style', name: '风格', icon: 'style', cmp: Style },
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
  const { currentTab, setCurrentTab, settingsOpen, setSettingsOpen } = useAppState(initialTab, initialSettings);
  const [settingsInitialSection, setSettingsInitialSection] = useState<SettingsSectionId | undefined>();
  const [providerPromptOpen, setProviderPromptOpen] = useState(false);
  const { hotkey } = useHotkeySettings();
  const Page = (NAV.find((n) => n.id === currentTab) ?? NAV[0]).cmp;

  useEffect(() => {
    let cancelled = false;
    (async () => {
      const credentials = await getCredentials();
      const promptSeenValue = window.localStorage.getItem(PROVIDER_SETUP_PROMPT_SEEN_KEY);
      if (!cancelled && shouldShowProviderSetupPrompt(credentials, promptSeenValue)) {
        setProviderPromptOpen(true);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const rememberProviderPrompt = () => {
    window.localStorage.setItem(PROVIDER_SETUP_PROMPT_SEEN_KEY, '1');
    setProviderPromptOpen(false);
  };

  const openSettings = (section?: SettingsSectionId) => {
    setSettingsInitialSection(section);
    setSettingsOpen(true);
  };

  const openProviderSettings = () => {
    rememberProviderPrompt();
    openSettings('提供商');
  };

  return (
    <div style={{ flex: 1, position: 'relative', display: 'flex', flexDirection: 'column', minHeight: 0, paddingTop: os === 'mac' ? 36 : 0 }}>

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

        {/* Sidebar — inside the raised console */}
        <aside
          style={{
            width: 188,
            flexShrink: 0,
            display: 'flex', flexDirection: 'column',
            background: 'linear-gradient(180deg, rgba(247,247,250,0.85) 0%, rgba(247,247,250,0.5) 100%)',
            padding: '14px 10px 12px',
          }}>

          {/* brand */}
          <div style={{ display: 'flex', alignItems: 'center', gap: 9, padding: '4px 8px 14px' }}>
            <img
              src="AppIcon.png"
              alt="OpenLess"
              style={{ width: 22, height: 22, borderRadius: 5, boxShadow: '0 1px 2px rgba(0,0,0,.1), 0 0 0 0.5px rgba(0,0,0,.06)' }} />

            <div style={{ fontSize: 13.5, fontWeight: 600, letterSpacing: '-0.01em', color: 'var(--ol-ink)' }}>OpenLess</div>
            <span style={{
              marginLeft: 'auto', padding: '1px 6px', fontSize: 9.5, fontWeight: 600,
              borderRadius: 4, background: 'rgba(0,0,0,0.06)', color: 'var(--ol-ink-3)',
              letterSpacing: '0.04em',
            }}>{APP_VERSION_LABEL}</span>
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
                    cursor: 'default', transition: 'background .12s, color .12s',
                    textAlign: 'left',
                  }}>

                  <Icon name={n.icon} size={14} />
                  <span style={{ flex: 1 }}>{n.name}</span>
                  {n.id === 'history' &&
                  <span style={{
                    fontSize: 10, fontFamily: 'var(--ol-font-mono)',
                    color: active ? 'var(--ol-ink-4)' : 'var(--ol-ink-5)',
                  }}>{OL_DATA.history.length}</span>
                  }
                </button>
              );
            })}
          </nav>

          <div style={{ flex: 1 }} />

          {/* shortcut hint */}
          <div
            style={{
              padding: '10px 10px 8px',
              borderTop: '0.5px dashed var(--ol-line)',
              marginTop: 8,
            }}>

            <div style={{ fontSize: 10.5, color: 'var(--ol-ink-4)', marginBottom: 6, letterSpacing: '0.02em' }}>录音快捷键</div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 11, color: 'var(--ol-ink-2)' }}>
              <kbd style={{
              padding: '2px 7px', fontSize: 10.5,
                background: '#fff', borderRadius: 5,
                border: '0.5px solid var(--ol-line-strong)',
                fontFamily: 'var(--ol-font-mono)', color: 'var(--ol-ink)',
                boxShadow: '0 1px 0 rgba(0,0,0,.04)',
              }}>{getHotkeyTriggerLabel(hotkey?.trigger)}</kbd>
              <span style={{ color: 'var(--ol-ink-4)' }}>开始 / 停止</span>
            </div>
          </div>

          {/* trial / status */}
          <div
            style={{
              marginTop: 10,
              padding: 12,
              borderRadius: 10,
              background: 'linear-gradient(160deg, rgba(37,99,235,0.08) 0%, rgba(37,99,235,0.02) 100%)',
              border: '0.5px solid rgba(37,99,235,0.15)',
            }}>

            <div style={{ fontSize: 10.5, fontWeight: 600, color: 'var(--ol-blue)', letterSpacing: '0.04em', textTransform: 'uppercase' }}>BETA</div>
            <div style={{ fontSize: 11.5, color: 'var(--ol-ink-2)', marginTop: 4, lineHeight: 1.5 }}>所有数据都只保存在本机。</div>
          </div>
        </aside>

        {/* Main content — inset white card sitting on the frosted backplate */}
        <div style={{ flex: 1, minWidth: 0, padding: '6px 8px 6px 0', display: 'flex' }}>
          <main
            style={{
              flex: 1, minWidth: 0,
              overflow: 'hidden',
              background: 'var(--ol-surface)',
              borderRadius: 12,
              border: '0.5px solid rgba(0,0,0,0.06)',
              boxShadow: '0 1px 0 rgba(255,255,255,0.8) inset, 0 8px 24px -12px rgba(15,17,22,0.10), 0 2px 6px -2px rgba(15,17,22,0.06)',
              display: 'flex',
              flexDirection: 'column',
            }}
          >
            <div style={{ padding: '24px 28px 32px', flex: 1, minHeight: 0, overflow: 'auto' }}>
              {currentTab === 'overview' ? (
                <Overview onOpenHistory={() => setCurrentTab('history')} />
              ) : (
                <Page />
              )}
            </div>
          </main>
        </div>
      </div>

      {/* Footer — sits on frosted outer, like Typeless */}
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

        <FooterIcon name="user" tip="账户" onClick={() => openSettings('提供商')} />
        <FooterIcon name="mail" tip="反馈" onClick={() => openExternal('https://github.com/appergb/openless/issues')} />
        <FooterIcon name="settings" tip="设置" active={settingsOpen} onClick={() => openSettings()} />
        <FooterIcon name="help" tip="帮助" onClick={() => openExternal('https://github.com/appergb/openless#readme')} />

        <div style={{ flex: 1 }} />

        <span style={{ fontFamily: 'var(--ol-font-sans)' }}>版本 {APP_VERSION_LABEL}</span>
        <button
          onClick={() => openExternal('https://github.com/appergb/openless/releases')}
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
          }}
        >
          检查更新
        </button>
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

      {providerPromptOpen && (
        <ProviderSetupPrompt
          onLater={rememberProviderPrompt}
          onOpenSettings={openProviderSettings}
        />
      )}
    </div>
  );
}

function ProviderSetupPrompt({ onLater, onOpenSettings }: { onLater: () => void; onOpenSettings: () => void }) {
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
        backdropFilter: 'blur(2px)',
        WebkitBackdropFilter: 'blur(2px)',
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
          <div style={{ fontSize: 14, fontWeight: 600, color: 'var(--ol-ink)' }}>设置语音提供商</div>
        </div>
        <div style={{ fontSize: 12.5, color: 'var(--ol-ink-3)', lineHeight: 1.55 }}>
          还没有配置 ASR 或 LLM 提供商，语音输入和润色暂时无法正常工作。
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
            }}
          >
            稍后
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
            }}
          >
            去设置
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
  return (
    <button
      onClick={onClick}
      title={tip}
      style={{
        width: 30, height: 30, borderRadius: 7, border: 0,
        background: active ? 'rgba(0,0,0,0.06)' : 'transparent',
        color: active ? 'var(--ol-ink)' : 'var(--ol-ink-4)',
        display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
        cursor: 'default',
      }}>
      <Icon name={name} size={15} />
    </button>
  );
}
