// WindowChrome.tsx — frosted outer frame + raised inner console pattern.
// The OUTER frame is a translucent shell with a tinted backdrop showing through.
// The INNER content lives in a single raised card that floats above it.
//
// Layout per window:
//   ┌─ frosted outer ───────────────────────────────┐
//   │ [titlebar]                                    │
//   │     ┌─ raised console (white, shadow) ─┐      │
//   │     │  sidebar │ main                  │      │
//   │     └──────────────────────────────────┘      │
//   │ [icon footer]                                 │
//   └───────────────────────────────────────────────┘

import { type CSSProperties, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';

export type OS = 'mac' | 'win';

export function detectOS(): OS {
  if (typeof navigator === 'undefined') return 'mac';
  const ua = navigator.userAgent || '';
  if (/Mac|iPhone|iPad|iPod/.test(ua)) return 'mac';
  if (/Windows/.test(ua)) return 'win';
  return 'mac';
}

const MAC_TITLEBAR_HEIGHT = 36;
const MAC_SYSTEM_CONTROLS_RESERVED_WIDTH = 80;
const MAC_WINDOW_RADIUS = 20;
const WIN_WINDOW_RADIUS = 0;

interface WindowChromeProps {
  os?: OS;
  title?: string;
  children: ReactNode;
  height?: number | string;
}

export function WindowChrome({ os = 'mac', title = 'OpenLess', children, height = 800 }: WindowChromeProps) {
  return (
    <div
      style={{
        width: '100%',
        height,
        position: 'relative',
        borderRadius: os === 'mac' ? MAC_WINDOW_RADIUS : WIN_WINDOW_RADIUS,
        boxShadow: 'var(--ol-shadow-xl)',
        overflow: 'hidden',
        display: 'flex',
        flexDirection: 'column',
        border: '0.5px solid rgba(0,0,0,.10)',
        background: `
          radial-gradient(120% 80% at 0% 0%, rgba(255,255,255,0.7) 0%, rgba(255,255,255,0) 60%),
          radial-gradient(100% 70% at 100% 100%, rgba(37,99,235,0.07) 0%, rgba(37,99,235,0) 55%),
          linear-gradient(180deg, rgba(245,245,247,0.92) 0%, rgba(232,232,236,0.92) 100%)
        `,
        backdropFilter: 'blur(40px) saturate(180%)',
        WebkitBackdropFilter: 'blur(40px) saturate(180%)',
      }}
    >
      {os === 'win' && <WinTitleBar title={title} />}
      {/* macOS：三色窗口按钮交给系统绘制和定位。这里只保留顶部拖动区，
          并避开系统按钮热区，防止拖动层吞掉 close/minimize/zoom 点击。 */}
      {os === 'mac' && (
        <div
          data-tauri-drag-region
          style={{
            position: 'absolute',
            top: 0,
            left: MAC_SYSTEM_CONTROLS_RESERVED_WIDTH,
            right: 0,
            height: MAC_TITLEBAR_HEIGHT,
            zIndex: 50,
          }}
        />
      )}
      <div style={{ flex: 1, minHeight: 0, display: 'flex', position: 'relative' }}>
        {children}
      </div>
    </div>
  );
}

interface WinTitleBarProps {
  title: string;
}

function WinTitleBar({ title }: WinTitleBarProps) {
  const { t } = useTranslation();
  return (
    <div
      style={{
        height: 36,
        flexShrink: 0,
        display: 'flex',
        alignItems: 'stretch',
        position: 'relative',
        zIndex: 5,
      }}
    >
      <div
        data-tauri-drag-region
        style={{ flex: 1, display: 'flex', alignItems: 'center', padding: '0 14px', gap: 10 }}
      >
        <img src="AppIcon.png" alt="" style={{ width: 14, height: 14, borderRadius: 3 }} />
        <span style={{ fontSize: 12, color: 'var(--ol-ink-3)', fontWeight: 500 }}>{title}</span>
      </div>
      <div style={{ display: 'flex' }}>
        <button style={winBtnStyle} title={t('windowChrome.minimize')} onClick={() => runWindowsWindowAction('minimize')}>
          <svg width="10" height="10" viewBox="0 0 10 10"><path d="M0 5h10" stroke="currentColor" strokeWidth="1" /></svg>
        </button>
        <button style={winBtnStyle} title={t('windowChrome.maximize')} onClick={() => runWindowsWindowAction('toggleMaximize')}>
          <svg width="10" height="10" viewBox="0 0 10 10"><rect x="0.5" y="0.5" width="9" height="9" stroke="currentColor" strokeWidth="1" fill="none" /></svg>
        </button>
        <button style={winCloseBtnStyle} title={t('windowChrome.close')} onClick={() => runWindowsWindowAction('close')}>
          <svg width="10" height="10" viewBox="0 0 10 10"><path d="M0 0L10 10M10 0L0 10" stroke="currentColor" strokeWidth="1" /></svg>
        </button>
      </div>
    </div>
  );
}

async function runWindowsWindowAction(action: 'minimize' | 'toggleMaximize' | 'close') {
  try {
    const { getCurrentWindow } = await import('@tauri-apps/api/window');
    const currentWindow = getCurrentWindow();
    if (action === 'minimize') {
      await currentWindow.minimize();
    } else if (action === 'toggleMaximize') {
      await currentWindow.toggleMaximize();
    } else {
      await currentWindow.close();
    }
  } catch (error) {
    console.warn(`[window] Windows title button ${action} failed`, error);
  }
}

const winBtnStyle: CSSProperties = {
  width: 46,
  height: '100%',
  border: 0,
  background: 'transparent',
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  color: 'var(--ol-ink-3)',
  cursor: 'default',
  transition: 'background 0.12s ease-out, color 0.12s ease-out',
};

const winCloseBtnStyle: CSSProperties = {
  ...winBtnStyle,
  color: 'var(--ol-ink-3)',
};
