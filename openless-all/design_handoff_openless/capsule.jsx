// capsule.jsx — the floating "recording" overlay that appears when the user
// hits the global hotkey. Compact pill: cancel · waveform · timer · confirm.
// macOS dark pill is used on both platforms (per design direction).

const { useEffect, useState } = React;

const Waveform = ({ bars = 18, active = true, accent = 'currentColor' }) => {
  const heights = React.useMemo(
    () => Array.from({ length: bars }, (_, i) => 0.25 + Math.abs(Math.sin(i * 0.9)) * 0.75),
    [bars]
  );
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 3, height: 22 }}>
      {heights.map((h, i) => (
        <span
          key={i}
          style={{
            display: 'inline-block',
            width: 2.5,
            height: `${h * 100}%`,
            borderRadius: 999,
            background: accent,
            opacity: 0.55 + h * 0.45,
            animation: active ? `wf ${0.6 + (i % 5) * 0.13}s ease-in-out ${i * 0.04}s infinite alternate` : 'none',
          }}
        />
      ))}
      <style>{`@keyframes wf { from { transform: scaleY(.4) } to { transform: scaleY(1.15) } }`}</style>
    </div>
  );
};

// Recording — pill with cancel · waveform · timer · confirm
const CapsuleMac = ({ recording = true, time = '0:08', onCancel, onConfirm }) => (
  <div
    style={{
      display: 'inline-flex',
      alignItems: 'center',
      gap: 10,
      padding: '8px 8px',
      borderRadius: 999,
      background: 'rgba(20, 20, 22, 0.78)',
      backdropFilter: 'blur(28px) saturate(180%)',
      WebkitBackdropFilter: 'blur(28px) saturate(180%)',
      boxShadow: '0 18px 50px -10px rgba(0,0,0,0.45), 0 0 0 0.5px rgba(255,255,255,0.08), inset 0 0.5px 0 rgba(255,255,255,0.10)',
      color: '#fff',
      fontFamily: 'var(--ol-font-sans)',
    }}
  >
    <button
      onClick={onCancel}
      aria-label="cancel"
      style={{
        width: 32, height: 32, borderRadius: 999,
        border: 0,
        background: 'rgba(255,255,255,0.10)',
        color: 'rgba(255,255,255,0.85)',
        display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
        cursor: 'default',
      }}
    >
      <svg width="11" height="11" viewBox="0 0 11 11"><path d="M1 1l9 9M10 1l-9 9" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" /></svg>
    </button>

    <div style={{ display: 'flex', alignItems: 'center', gap: 10, padding: '0 4px' }}>
      <Waveform bars={20} active={recording} accent="#fbbf24" />
      <span style={{
        fontVariantNumeric: 'tabular-nums', fontSize: 12,
        color: 'rgba(255,255,255,0.7)', minWidth: 30, textAlign: 'right',
        fontFamily: 'var(--ol-font-mono)',
      }}>{time}</span>
    </div>

    <button
      onClick={onConfirm}
      aria-label="confirm"
      style={{
        width: 32, height: 32, borderRadius: 999,
        border: 0,
        background: '#fff',
        color: '#0a0a0b',
        display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
        cursor: 'default',
        boxShadow: '0 0 0 3px rgba(255,255,255,0.10)',
      }}
    >
      <svg width="13" height="13" viewBox="0 0 13 13"><path d="M2 6.5l3.2 3.5L11 3.5" stroke="currentColor" strokeWidth="1.6" fill="none" strokeLinecap="round" strokeLinejoin="round" /></svg>
    </button>
  </div>
);

// Transcribing — waveform freezes; spinner + label between cancel & confirm slots
const CapsuleTranscribing = () => (
  <div
    style={{
      display: 'inline-flex',
      alignItems: 'center',
      gap: 10,
      padding: '8px 8px',
      borderRadius: 999,
      background: 'rgba(20, 20, 22, 0.78)',
      backdropFilter: 'blur(28px) saturate(180%)',
      WebkitBackdropFilter: 'blur(28px) saturate(180%)',
      boxShadow: '0 18px 50px -10px rgba(0,0,0,0.45), 0 0 0 0.5px rgba(255,255,255,0.08), inset 0 0.5px 0 rgba(255,255,255,0.10)',
      color: '#fff',
      fontFamily: 'var(--ol-font-sans)',
    }}
  >
    <span style={{
      width: 32, height: 32, borderRadius: 999,
      background: 'rgba(255,255,255,0.10)',
    }} />
    <div style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '0 6px' }}>
      <span style={{
        width: 14, height: 14, borderRadius: 999,
        border: '1.5px solid rgba(255,255,255,0.30)',
        borderTopColor: '#fff',
        animation: 'ol-cap-spin 0.7s linear infinite',
      }} />
      <span style={{ fontSize: 12, color: 'rgba(255,255,255,0.75)' }}>转写中</span>
    </div>
    <span style={{
      width: 32, height: 32, borderRadius: 999, background: '#fff',
    }} />
    <style>{`@keyframes ol-cap-spin { to { transform: rotate(360deg) } }`}</style>
  </div>
);

// Done — momentary blue confirmation, fades out
const CapsuleDone = ({ chars = 56 }) => (
  <div
    style={{
      display: 'inline-flex',
      alignItems: 'center',
      gap: 8,
      padding: '8px 16px',
      borderRadius: 999,
      background: 'var(--ol-blue)',
      color: '#fff',
      boxShadow: '0 12px 30px -8px rgba(37,99,235,.4)',
      fontSize: 12, fontWeight: 500,
      fontFamily: 'var(--ol-font-sans)',
    }}
  >
    <svg width="14" height="14" viewBox="0 0 14 14"><path d="M2 7l3.5 3.5L12 3.5" stroke="currentColor" strokeWidth="1.7" fill="none" strokeLinecap="round" strokeLinejoin="round" /></svg>
    已插入 {chars} 字
  </div>
);

// Both platforms use the same capsule
const CapsuleWin = CapsuleMac;
const Capsule = CapsuleMac;

window.CapsuleMac = CapsuleMac;
window.CapsuleWin = CapsuleWin;
window.Capsule = Capsule;
window.CapsuleTranscribing = CapsuleTranscribing;
window.CapsuleDone = CapsuleDone;
window.Waveform = Waveform;
