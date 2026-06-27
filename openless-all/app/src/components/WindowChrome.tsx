import { type CSSProperties, type ReactNode, useCallback, useEffect, useRef, useState } from 'react';

export type OS = 'mac' | 'win' | 'linux' | 'android';

export function detectOS(): OS {
  if (typeof navigator === 'undefined') return 'mac';
  const uaDataPlatform = (
    navigator as Navigator & { userAgentData?: { platform?: string } }
  ).userAgentData?.platform ?? '';
  const hints = `${navigator.userAgent || ''} ${navigator.platform || ''} ${uaDataPlatform}`;
  if (/Mac|iPhone|iPad|iPod/.test(hints)) return 'mac';
  if (/Android/i.test(hints)) return 'android';
  if (/Windows|Win32|Win64/.test(hints)) return 'win';
  if (/Linux|X11|Wayland/.test(hints)) return 'linux';
  return 'mac';
}

const MAC_TITLEBAR_HEIGHT = 28;
const MAC_SYSTEM_CONTROLS_RESERVED_WIDTH = 76;
const LINUX_TITLEBAR_HEIGHT = 36;
const WIN_CONSOLE_RADIUS = 10;

interface WindowChromeProps {
  os?: OS;
  title?: string;
  children: ReactNode;
  height?: number | string;
}

export function WindowChrome({
  os = 'mac',
  children,
  height = 800,
}: WindowChromeProps) {
  // Windows: decorations:true 时外层不画圆角/边框/阴影/标题栏，避免与原生窗口重叠。
  // Linux: decorations:false 时外层画 14px 圆角 + 自定义标题栏。
  const shellRadius = os === 'mac' ? 0 : os === 'win' || os === 'android' ? 0 : 14;
  const consoleRadius = os === 'mac' ? 20 : os === 'win' ? WIN_CONSOLE_RADIUS : os === 'android' ? 0 : 14;
  const titlebarHeight = os === 'mac' ? MAC_TITLEBAR_HEIGHT : os === 'linux' ? LINUX_TITLEBAR_HEIGHT : 0;

  const useSolidSurface = os === 'linux' || os === 'android';

  return (
    <div
      className="ol-winchrome"
      style={{
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
        border: os === 'win' ? 'none' : os === 'mac' ? 'none' : '0.5px solid var(--ol-window-border)',
        background: useSolidSurface ? 'var(--ol-surface)' : 'var(--ol-window-bg)',
        backdropFilter: useSolidSurface ? 'none' : 'blur(var(--ol-glass-blur-strong)) saturate(190%)',
        WebkitBackdropFilter: useSolidSurface ? 'none' : 'blur(var(--ol-glass-blur-strong)) saturate(190%)',
        animation: os === 'win' ? undefined : 'ol-window-enter 0.42s var(--ol-motion-spring) both',
        transition: 'box-shadow 0.28s var(--ol-motion-soft), border-color 0.28s var(--ol-motion-soft), backdrop-filter 0.28s var(--ol-motion-soft)',
        willChange: 'opacity, transform, filter',
      } as CSSProperties}
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
      {os === 'linux' && <LinuxTitlebar />}
      {os === 'linux' && (
        <style>{`.ol-linux-close-btn:hover{background:rgba(220,38,38,0.12)!important;color:rgb(220,38,38)!important}`}</style>
      )}
      <div style={{ flex: 1, minHeight: 0, display: 'flex', position: 'relative' }}>
        {children}
      </div>
    </div>
  );
}

// ── Linux custom titlebar — mirrors cc-switch's approach ──

type TauriWindow = import('@tauri-apps/api/window').Window;

function LinuxTitlebar() {
  const [maximized, setMaximized] = useState(false);
  const winRef = useRef<TauriWindow | null>(null);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    import('@tauri-apps/api/window').then(({ getCurrentWindow }) => {
      if (cancelled) return;
      const w = getCurrentWindow();
      winRef.current = w;
      w.isMaximized().then((m) => {
        if (!cancelled) setMaximized(m);
      }).catch(() => {});
      // Keep icon in sync when user maximizes via double-click / keyboard shortcut
      w.listen('tauri://resize', () => {
        if (cancelled) return;
        w.isMaximized().then((m) => {
          if (!cancelled) setMaximized(m);
        }).catch(() => {});
      }).then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      }).catch(() => {});
    }).catch(() => {});
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  const onMinimize = useCallback(() => {
    winRef.current?.minimize().catch(() => {});
  }, []);

  const onToggleMaximize = useCallback(() => {
    const w = winRef.current;
    if (!w) return;
    w.toggleMaximize().catch(() => {});
    // Re-query after window manager processes the toggle, in case WM rejects it
    setTimeout(() => {
      w.isMaximized().then(setMaximized).catch(() => {});
    }, 300);
  }, []);

  const onClose = useCallback(() => {
    winRef.current?.close().catch(() => {});
  }, []);

  return (
    <div
      data-tauri-drag-region
      className="ol-linux-titlebar"
      style={{
        height: LINUX_TITLEBAR_HEIGHT,
        flexShrink: 0,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        padding: '0 6px 0 14px',
        background: 'var(--ol-surface)',
        backdropFilter: 'none',
        WebkitBackdropFilter: 'none',
        borderBottom: '0.5px solid rgba(0,0,0,0.08)',
        color: 'var(--ol-ink-3)',
        fontSize: 13,
        fontWeight: 500,
        cursor: 'default',
        userSelect: 'none',
        zIndex: 50,
      }}
    >
      <span style={{ color: 'var(--ol-ink-2)' }}>OpenLess</span>
      <div
        style={{ display: 'flex', gap: 4, pointerEvents: 'auto' }}
        onMouseDown={(e) => e.stopPropagation()}
      >
        <button onClick={onMinimize} aria-label="Minimize" style={ctrlBtn}>
          <MinimizeSvg />
        </button>
        <button onClick={onToggleMaximize} aria-label={maximized ? 'Restore' : 'Maximize'} style={ctrlBtn}>
          {maximized ? <RestoreSvg /> : <MaximizeSvg />}
        </button>
        <button
          onClick={onClose}
          aria-label="Close"
          className="ol-linux-close-btn"
          style={ctrlBtn}
        >
          <CloseSvg />
        </button>
      </div>
    </div>
  );
}

// ── inline SVG icons (no lucide-react dep) ──

const svgWrap: CSSProperties = { width: 12, height: 12, display: 'block' };
const ctrlBtn: CSSProperties = {
  width: 30, height: 24,
  display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
  borderRadius: 5, border: 0, padding: 0,
  background: 'transparent', color: 'var(--ol-ink-3)',
  fontFamily: 'inherit', cursor: 'default',
  transition: 'background 0.12s, color 0.12s',
};

function MinimizeSvg() {
  return (
    <svg width="12" height="12" viewBox="0 0 12 12" fill="none" style={svgWrap}>
      <rect x="2" y="5.5" width="8" height="1" rx="0.5" fill="currentColor" />
    </svg>
  );
}

function MaximizeSvg() {
  return (
    <svg width="12" height="12" viewBox="0 0 12 12" fill="none" style={svgWrap}>
      <rect x="2" y="2" width="8" height="8" rx="1.5" stroke="currentColor" strokeWidth="1.1" />
    </svg>
  );
}

function RestoreSvg() {
  return (
    <svg width="12" height="12" viewBox="0 0 12 12" fill="none" style={svgWrap}>
      <rect x="3.6" y="0.6" width="7.2" height="7.2" rx="1.3" stroke="currentColor" strokeWidth="1.1" />
      <rect x="0.6" y="3.6" width="7.2" height="7.2" rx="1.3" fill="var(--ol-surface, #fff)" stroke="currentColor" strokeWidth="1.1" />
    </svg>
  );
}

function CloseSvg() {
  return (
    <svg width="12" height="12" viewBox="0 0 12 12" fill="none" style={svgWrap}>
      <path d="M2.8 2.8l6.4 6.4M9.2 2.8l-6.4 6.4" stroke="currentColor" strokeWidth="1.25" strokeLinecap="round" />
    </svg>
  );
}
