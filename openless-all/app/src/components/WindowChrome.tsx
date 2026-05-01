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

import { useState, type CSSProperties, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { getCurrentWindow } from '@tauri-apps/api/window';

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
const WIN_RESIZE_EDGE = 6;
const WIN_RESIZE_CORNER = 14;

type ResizeDirection =
  | 'East'
  | 'North'
  | 'NorthEast'
  | 'NorthWest'
  | 'South'
  | 'SouthEast'
  | 'SouthWest'
  | 'West';

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
      {os === 'win' && <WindowsResizeHandles />}
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
        zIndex: 70,
      }}
    >
      <div
        data-tauri-drag-region
        style={{ flex: 1, display: 'flex', alignItems: 'center', padding: '0 14px', gap: 10 }}
      >
        <img src="AppIcon.png" alt="" style={{ width: 14, height: 14, borderRadius: 3 }} />
        <span style={{ fontSize: 12, color: 'var(--ol-ink-3)', fontWeight: 500 }}>{title}</span>
      </div>
      <div style={{ display: 'flex', pointerEvents: 'auto' }}>
        <WinTitleButton title={t('windowChrome.minimize')} action="minimize">
          <svg width="10" height="10" viewBox="0 0 10 10"><path d="M0 5h10" stroke="currentColor" strokeWidth="1" /></svg>
        </WinTitleButton>
        <WinTitleButton title={t('windowChrome.maximize')} action="toggleMaximize">
          <svg width="10" height="10" viewBox="0 0 10 10"><rect x="0.5" y="0.5" width="9" height="9" stroke="currentColor" strokeWidth="1" fill="none" /></svg>
        </WinTitleButton>
        <WinTitleButton title={t('windowChrome.close')} action="close" tone="danger">
          <svg width="10" height="10" viewBox="0 0 10 10"><path d="M0 0L10 10M10 0L0 10" stroke="currentColor" strokeWidth="1" /></svg>
        </WinTitleButton>
      </div>
    </div>
  );
}

function WindowsResizeHandles() {
  const handles: Array<{
    direction: ResizeDirection;
    cursor: CSSProperties['cursor'];
    style: CSSProperties;
  }> = [
    { direction: 'North', cursor: 'ns-resize', style: { top: 0, left: WIN_RESIZE_CORNER, right: WIN_RESIZE_CORNER, height: WIN_RESIZE_EDGE } },
    { direction: 'South', cursor: 'ns-resize', style: { bottom: 0, left: WIN_RESIZE_CORNER, right: WIN_RESIZE_CORNER, height: WIN_RESIZE_EDGE } },
    { direction: 'West', cursor: 'ew-resize', style: { top: WIN_RESIZE_CORNER, bottom: WIN_RESIZE_CORNER, left: 0, width: WIN_RESIZE_EDGE } },
    { direction: 'East', cursor: 'ew-resize', style: { top: WIN_RESIZE_CORNER, bottom: WIN_RESIZE_CORNER, right: 0, width: WIN_RESIZE_EDGE } },
    { direction: 'NorthWest', cursor: 'nwse-resize', style: { top: 0, left: 0, width: WIN_RESIZE_CORNER, height: WIN_RESIZE_CORNER } },
    { direction: 'NorthEast', cursor: 'nesw-resize', style: { top: 0, right: 0, width: WIN_RESIZE_CORNER, height: WIN_RESIZE_CORNER } },
    { direction: 'SouthWest', cursor: 'nesw-resize', style: { bottom: 0, left: 0, width: WIN_RESIZE_CORNER, height: WIN_RESIZE_CORNER } },
    { direction: 'SouthEast', cursor: 'nwse-resize', style: { bottom: 0, right: 0, width: WIN_RESIZE_CORNER, height: WIN_RESIZE_CORNER } },
  ];

  return (
    <div aria-hidden style={{ position: 'absolute', inset: 0, pointerEvents: 'none', zIndex: 60 }}>
      {handles.map(handle => (
        <div
          key={handle.direction}
          onMouseDown={event => {
            if (event.button !== 0) return;
            event.preventDefault();
            event.stopPropagation();
            void startWindowsResize(handle.direction);
          }}
          style={{
            position: 'absolute',
            pointerEvents: 'auto',
            cursor: handle.cursor,
            ...handle.style,
          }}
        />
      ))}
    </div>
  );
}

interface WinTitleButtonProps {
  title: string;
  action: 'minimize' | 'toggleMaximize' | 'close';
  children: ReactNode;
  tone?: 'default' | 'danger';
}

function WinTitleButton({ title, action, children, tone = 'default' }: WinTitleButtonProps) {
  const [hovered, setHovered] = useState(false);
  const [pressed, setPressed] = useState(false);
  const danger = tone === 'danger';
  const background = pressed
    ? danger ? '#c42b1c' : 'rgba(0,0,0,0.12)'
    : hovered ? danger ? '#e81123' : 'rgba(0,0,0,0.08)'
      : 'transparent';
  const color = danger && (hovered || pressed) ? '#fff' : 'var(--ol-ink-3)';

  return (
    <button
      style={{ ...winBtnStyle, background, color }}
      title={title}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => {
        setHovered(false);
        setPressed(false);
      }}
      onMouseDown={() => setPressed(true)}
      onMouseUp={() => setPressed(false)}
      onClick={() => runWindowsWindowAction(action)}
    >
      {children}
    </button>
  );
}

async function startWindowsResize(direction: ResizeDirection) {
  try {
    await getCurrentWindow().startResizeDragging(direction);
  } catch (error) {
    console.warn(`[window] Windows resize ${direction} failed`, error);
  }
}

async function runWindowsWindowAction(action: 'minimize' | 'toggleMaximize' | 'close') {
  try {
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
