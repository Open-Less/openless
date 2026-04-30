import { useEffect, useState } from 'react';
import { Capsule } from './components/Capsule';
import { FloatingShell } from './components/FloatingShell';
import { Onboarding } from './components/Onboarding';
import { detectOS } from './components/WindowChrome';
import { checkAccessibilityPermission, checkMicrophonePermission, isTauri } from './lib/ipc';
import { HotkeySettingsProvider } from './state/HotkeySettingsContext';

interface AppProps {
  isCapsule: boolean;
}

type Gate = 'checking' | 'onboarding' | 'ready';

export function App({ isCapsule }: AppProps) {
  if (isCapsule) {
    return <Capsule />;
  }

  const os = detectOS();
  // Windows 启动不应被权限探测阻塞首屏。
  const [gate, setGate] = useState<Gate>(isTauri && os !== 'win' ? 'checking' : 'ready');

  useEffect(() => {
    if (!isTauri) return;
    let cancelled = false;
    requestAnimationFrame(() => {
      if (cancelled) return;
      import('@tauri-apps/api/window')
        .then(({ getCurrentWindow }) => getCurrentWindow().show())
        .catch(error => console.warn('[startup] show main window failed', error));
    });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (!isTauri || os === 'win') return;
    let cancelled = false;
    (async () => {
      const [a, m] = await Promise.all([
        checkAccessibilityPermission(),
        checkMicrophonePermission(),
      ]);
      if (cancelled) return;
      const aOk = a === 'granted' || a === 'notApplicable';
      const mOk = m === 'granted' || m === 'notApplicable';
      setGate(aOk && mOk ? 'ready' : 'onboarding');
    })();
    return () => {
      cancelled = true;
    };
  }, [os]);

  if (gate === 'checking') {
    return <StartupShell />;
  }
  return (
    <HotkeySettingsProvider>
      {gate === 'onboarding' ? <Onboarding onComplete={() => setGate('ready')} /> : <FloatingShell />}
    </HotkeySettingsProvider>
  );
}

function StartupShell() {
  return (
    <div
      style={{
        minHeight: '100vh',
        display: 'grid',
        placeItems: 'center',
        background: 'linear-gradient(180deg, rgba(245,245,247,0.96) 0%, rgba(232,232,236,0.96) 100%)',
        color: 'var(--ol-ink-3)',
        fontFamily: 'var(--ol-font-sans)',
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', gap: 10, fontSize: 13, fontWeight: 500 }}>
        <img src="AppIcon.png" alt="" style={{ width: 18, height: 18, borderRadius: 4 }} />
        <span>OpenLess 正在启动</span>
      </div>
    </div>
  );
}
