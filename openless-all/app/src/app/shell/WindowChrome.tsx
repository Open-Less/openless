import { type CSSProperties, type ReactNode } from 'react';

import { ViewportPortal } from '../../ui/layout/AppViewport';
import type { OS } from '../../runtime/environment';

const MAC_TITLEBAR_HEIGHT = 44;
const MAC_SYSTEM_CONTROLS_RESERVED_WIDTH = 88;
const WIN_CONSOLE_RADIUS = 10;

interface WindowChromeProps {
  os?: OS;
  mobile: boolean;
  title?: string;
  children: ReactNode;
  height?: number | string;
}

export function WindowChrome({ os = 'mac', mobile, children, height = 800 }: WindowChromeProps) {
  // Windows: with decorations:true the shell draws no radius/border/shadow/titlebar,
  // avoiding overlap with the native window.
  const shellRadius =
    os === 'mac' ? 0 : os === 'win' || os === 'android' || os === 'harmony' ? 0 : 14;
  const consoleRadius = os === 'mac' ? 20 : os === 'win' ? WIN_CONSOLE_RADIUS : 0;
  // The compact shell puts live controls in the strip reserved for desktop window drag.
  const hasMacDragStrip = os === 'mac' && !mobile;
  const titlebarHeight = hasMacDragStrip ? MAC_TITLEBAR_HEIGHT : 0;

  const useSolidSurface = os === 'android' || os === 'harmony';

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
          isolation: 'isolate',
          borderRadius: 'var(--ol-window-shell-radius)',
          // The shell fills the whole window, so a CSS shadow lands outside the viewport and
          // is clipped; macOS/Windows draw the real window shadow natively.
          boxShadow: os === 'win' || os === 'mac' ? 'none' : 'var(--ol-shadow-xl)',
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
            os === 'win' ? undefined : 'ol-window-enter 0.24s var(--ol-motion-spring) backwards',
          transition:
            'box-shadow 0.28s var(--ol-motion-soft), border-color 0.28s var(--ol-motion-soft)',
        } as CSSProperties
      }
    >
      {hasMacDragStrip && (
        <ViewportPortal>
          <div
            data-tauri-drag-region
            style={{
              position: 'absolute',
              top: 0,
              left: MAC_SYSTEM_CONTROLS_RESERVED_WIDTH,
              right: 0,
              height: titlebarHeight,
              pointerEvents: 'auto',
              // Dragging uses unscaled native coordinates above the settings workspace.
              zIndex: 100,
            }}
          />
        </ViewportPortal>
      )}
      <div style={{ flex: 1, minHeight: 0, display: 'flex', position: 'relative' }}>{children}</div>
    </div>
  );
}
