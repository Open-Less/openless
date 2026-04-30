import { useEffect, useState } from 'react';
import { Capsule } from './components/Capsule';
import { FloatingShell } from './components/FloatingShell';
import { Onboarding } from './components/Onboarding';
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

  // 浏览器 dev 时跳过权限检查；只有真正在 Tauri 里才门控。
  const [gate, setGate] = useState<Gate>(isTauri ? 'checking' : 'ready');

  useEffect(() => {
    if (!isTauri) return;
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
  }, []);

  if (gate === 'checking') {
    return null;
  }
  return (
    <HotkeySettingsProvider>
      {gate === 'onboarding' ? <Onboarding onComplete={() => setGate('ready')} /> : <FloatingShell />}
    </HotkeySettingsProvider>
  );
}
