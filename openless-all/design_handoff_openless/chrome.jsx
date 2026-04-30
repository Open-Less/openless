// chrome.jsx — frosted outer frame + raised inner console pattern.
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

const WindowChrome = ({ os = 'mac', title = 'OpenLess', children, height = 800 }) => {
  return (
    <div
      style={{
        width: '100%',
        height,
        position: 'relative',
        borderRadius: os === 'mac' ? 20 : 14,
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
      <div style={{ flex: 1, minHeight: 0, display: 'flex', position: 'relative' }}>
        {children}
      </div>
      {/* macOS traffic lights float above everything, no titlebar bar */}
      {os === 'mac' && <MacTrafficLights />}
    </div>
  );
};

const MacTrafficLights = () => {
  const [hover, setHover] = React.useState(false);
  return (
    <div
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        position: 'absolute',
        top: 13, left: 14,
        display: 'flex', gap: 8, alignItems: 'center',
        zIndex: 100,
      }}
    >
      <TrafficDot color="#ff5f57" hover={hover} icon="close" />
      <TrafficDot color="#febc2e" hover={hover} icon="min" />
      <TrafficDot color="#28c840" hover={hover} icon="max" />
    </div>
  );
};

const TrafficDot = ({ color, hover, icon }) => {
  // Reveal symbol on hover, like real macOS traffic lights
  const symbols = {
    close: <path d="M3 3l4 4M7 3l-4 4" stroke="rgba(0,0,0,0.6)" strokeWidth="1.2" strokeLinecap="round" />,
    min:   <path d="M2.5 5h5" stroke="rgba(0,0,0,0.6)" strokeWidth="1.2" strokeLinecap="round" />,
    max:   <path d="M3 4l2-2 2 2M3 6l2 2 2-2" stroke="rgba(0,0,0,0.55)" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round" fill="none" />,
  };
  return (
    <button
      style={{
        width: 12, height: 12, borderRadius: 999,
        background: color,
        border: 0,
        boxShadow: 'inset 0 0 0 0.5px rgba(0,0,0,.18)',
        display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
        padding: 0, cursor: 'default',
      }}
      title={icon}
      aria-label={icon}
    >
      <svg width="10" height="10" viewBox="0 0 10 10" style={{ opacity: hover ? 1 : 0, transition: 'opacity .12s' }}>
        {symbols[icon]}
      </svg>
    </button>
  );
};

const WinTitleBar = ({ title }) => (
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
    <div style={{ flex: 1, display: 'flex', alignItems: 'center', padding: '0 14px', gap: 10 }}>
      <img src="AppIcon.png" alt="" style={{ width: 14, height: 14, borderRadius: 3 }} />
      <span style={{ fontSize: 12, color: 'var(--ol-ink-3)', fontWeight: 500 }}>{title}</span>
    </div>
    <div style={{ display: 'flex' }}>
      <button style={winBtnStyle}>
        <svg width="10" height="10" viewBox="0 0 10 10"><path d="M0 5h10" stroke="currentColor" strokeWidth="1" /></svg>
      </button>
      <button style={winBtnStyle}>
        <svg width="10" height="10" viewBox="0 0 10 10"><rect x="0.5" y="0.5" width="9" height="9" stroke="currentColor" strokeWidth="1" fill="none" /></svg>
      </button>
      <button style={winBtnStyle}>
        <svg width="10" height="10" viewBox="0 0 10 10"><path d="M0 0L10 10M10 0L0 10" stroke="currentColor" strokeWidth="1" /></svg>
      </button>
    </div>
  </div>
);

const winBtnStyle = {
  width: 46,
  height: '100%',
  border: 0,
  background: 'transparent',
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  color: 'var(--ol-ink-3)',
  cursor: 'default',
};

window.WindowChrome = WindowChrome;
