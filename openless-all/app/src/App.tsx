import { useEffect, useState } from 'react';
import { Capsule } from './components/Capsule';
import { FloatingShell } from './components/FloatingShell';
import { Onboarding } from './components/Onboarding';
import { detectOS } from './components/WindowChrome';
import {
  checkAccessibilityPermission,
  checkMicrophonePermission,
  handleWindowHotkeyEvent,
  isTauri,
} from './lib/ipc';
import { QaPanel } from './pages/QaPanel';
import { HotkeySettingsProvider } from './state/HotkeySettingsContext';

interface AppProps {
  isCapsule: boolean;
  isQa: boolean;
}

type Gate = 'checking' | 'onboarding' | 'ready';

export function App({ isCapsule, isQa }: AppProps) {
  if (isCapsule) {
    return <Capsule />;
  }
  if (isQa) {
    return <QaPanel />;
  }

  const os = detectOS();
  const [gate, setGate] = useState<Gate>(isTauri && os !== 'win' ? 'checking' : 'ready');

  useEffect(() => {
    if (!isTauri) return;
    if (os === 'win') return;
    let cancelled = false;
    requestAnimationFrame(() => {
      if (cancelled) return;
      import('@tauri-apps/api/window')
        .then(async ({ getCurrentWindow }) => {
          const currentWindow = getCurrentWindow();
          if (!(await currentWindow.isVisible())) {
            await currentWindow.show();
          }
        })
        .catch(error => console.warn('[startup] show main window failed', error));
    });
    return () => {
      cancelled = true;
    };
  }, [os]);

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

  useEffect(() => {
    if (!isTauri || os !== 'win') return;
    const forwardKey = (event: KeyboardEvent) => {
      if (!isWindowHotkeyCandidate(event)) return;
      void handleWindowHotkeyEvent(
        event.type as 'keydown' | 'keyup',
        event.key,
        event.code,
        event.repeat,
      ).catch(error => console.warn('[window-hotkey] forward failed', error));
    };
    window.addEventListener('keydown', forwardKey, true);
    window.addEventListener('keyup', forwardKey, true);
    return () => {
      window.removeEventListener('keydown', forwardKey, true);
      window.removeEventListener('keyup', forwardKey, true);
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

function isWindowHotkeyCandidate(event: KeyboardEvent): boolean {
  return (
    event.key === 'Escape' ||
    event.code === 'ControlRight' ||
    event.code === 'ControlLeft' ||
    event.code === 'AltRight' ||
    event.code === 'MetaRight'
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
