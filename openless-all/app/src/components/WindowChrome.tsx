import {
  type CSSProperties,
  type ReactNode,
} from 'react';

export type OS = 'mac' | 'win' | 'android';

export function detectOS(): OS {
  if (typeof navigator === 'undefined') return 'mac';
  const uaDataPlatform =
    (navigator as Navigator & { userAgentData?: { platform?: string } }).userAgentData?.platform ??
    '';
  const hints = `${navigator.userAgent || ''} ${navigator.platform || ''} ${uaDataPlatform}`;
  if (/Mac|iPhone|iPad|iPod/.test(hints)) return 'mac';
  if (/Android/i.test(hints)) return 'android';
  if (/Windows|Win32|Win64/.test(hints)) return 'win';
  return 'mac';
}

const MAC_TITLEBAR_HEIGHT = 44;
const MAC_SYSTEM_CONTROLS_RESERVED_WIDTH = 88;
const WIN_CONSOLE_RADIUS = 10;

interface WindowChromeProps {
  os?: OS;
  title?: string;
  children: ReactNode;
  height?: number | string;
}

export function WindowChrome({ os = 'mac', children, height = 800 }: WindowChromeProps) {
  // Windows: decorations:true 时外层不画圆角/边框/阴影/标题栏，避免与原生窗口重叠。
  const shellRadius = 0;
  const consoleRadius = os === 'mac' ? 20 : os === 'win' ? WIN_CONSOLE_RADIUS : 0;
  const titlebarHeight = os === 'mac' ? MAC_TITLEBAR_HEIGHT : 0;

  const useSolidSurface = os === 'android';

  return (
    <div
      className="ol-winchrome"
      style={
        {
          '--ol-window-shell-radius': `${shellRadius}px`,
          '--ol-window-console-radius': `${consoleRadius}px`,
          '--ol-window-titlebar-height': `${titlebarHeight}px`,
          width: '100%',
          height,
          position: 'relative',
          borderRadius: 'var(--ol-window-shell-radius)',
          boxShadow: os === 'win' ? 'none' : 'var(--ol-shadow-xl)',
          overflow: 'hidden',
          display: 'flex',
          flexDirection: 'column',
          border:
            os === 'win' ? 'none' : os === 'mac' ? 'none' : '0.5px solid var(--ol-window-border)',
          background: useSolidSurface ? 'var(--ol-surface)' : 'var(--ol-window-bg)',
          // The main window is opaque; backdrop blur would only add a compositing layer.
          backdropFilter: 'none',
          WebkitBackdropFilter: 'none',
          animation:
            os === 'win' ? undefined : 'ol-window-enter 0.42s var(--ol-motion-spring) both',
          transition:
            'box-shadow 0.28s var(--ol-motion-soft), border-color 0.28s var(--ol-motion-soft)',
          willChange: 'opacity, transform',
        } as CSSProperties
      }
    >
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
      <div style={{ flex: 1, minHeight: 0, display: 'flex', position: 'relative' }}>{children}</div>
    </div>
  );
}

