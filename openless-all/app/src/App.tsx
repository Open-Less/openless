import { useEffect, useState } from 'react';
import { Capsule } from './components/Capsule';
import { FloatingShell } from './components/FloatingShell';
import { Onboarding } from './components/Onboarding';
import { detectOS } from './components/WindowChrome';
import {
  checkAccessibilityPermission,
  checkMicrophonePermission,
  getHotkeyStatus,
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
  // Windows 启动不应被权限探测阻塞首屏。
  const [gate, setGate] = useState<Gate>(isTauri ? 'checking' : 'ready');

  useEffect(() => {
    if (!isTauri) return;
    if (os === 'win' && gate === 'checking') return;
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
  }, [gate, os]);

  useEffect(() => {
    if (!isTauri) return;
    let cancelled = false;

    if (os === 'win') {
      // 超时保护：50 次 × 200ms = 10s。hotkey hook 永远 starting（被反作弊 / EDR
      // / UAC 拦）时不让 UI 死锁灰屏，过 10s 强 setGate('ready') 让用户进
      // Permissions 页看 hotkey_status.lastError 处理。详见 issue #163。
      const POLL_INTERVAL_MS = 200;
      const POLL_MAX_ATTEMPTS = 50;
      const pollHotkeyStatus = async () => {
        let attempts = 0;
        while (!cancelled && attempts < POLL_MAX_ATTEMPTS) {
          attempts += 1;
          const status = await getHotkeyStatus();
          if (cancelled) return;
          if (status.state !== 'starting') {
            setGate('ready');
            return;
          }
          await new Promise(resolve => window.setTimeout(resolve, POLL_INTERVAL_MS));
        }
        if (!cancelled) {
          console.warn(
            `[startup] hotkey gate timed out after ${POLL_MAX_ATTEMPTS * POLL_INTERVAL_MS}ms; forcing ready so user can reach Permissions page`
          );
          setGate('ready');
        }
      };
      void pollHotkeyStatus().catch(error => {
        console.warn('[startup] hotkey status polling failed', error);
        if (!cancelled) {
          setGate('ready');
        }
      });
      return () => {
        cancelled = true;
      };
    }

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
    // Windows 听写 / QA lifecycle 由 backend low-level keyboard hook 单独拥有。
    // 不再让前台 main window 额外转发 keydown/keyup，避免双事件源共同驱动同一状态机。
    return;
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
