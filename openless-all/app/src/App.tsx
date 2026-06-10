import { useEffect, useState } from 'react';
import { AutoUpdateGate } from './components/AutoUpdateGate';
import { Capsule } from './components/Capsule';
import { FloatingShell } from './components/FloatingShell';
import { Onboarding } from './components/Onboarding';
import { detectOS, type OS } from './components/WindowChrome';
import {
  checkAccessibilityPermission,
  checkMicrophonePermission,
  getHotkeyStatus,
  getSettings,
  getPlatformCapabilities,
  handleWindowHotkeyEvent,
  isTauri,
  qaWindowDismiss,
} from './lib/ipc';
import type { PlatformCapabilities } from './lib/types';
import {
  isWindowHotkeyKeyboardCandidate,
  windowMouseHotkeyCode,
} from './lib/windowHotkeyFallback';
import { QaPanel } from './pages/QaPanel';
import { LessComputerPanel } from './pages/LessComputerPanel';
import { LessComputerGlow } from './pages/LessComputerGlow';
import { HotkeySettingsProvider } from './state/HotkeySettingsContext';
import { APP_THEME_KEY, applyAppTheme, readAppTheme } from './lib/appTheme';

interface AppProps {
  isCapsule: boolean;
  isQa: boolean;
  isLessComputer: boolean;
  isLessComputerGlow: boolean;
  forcedOs?: OS | null;
}

type Gate = 'onboarding' | 'ready';

export function App({ isCapsule, isQa, isLessComputer, isLessComputerGlow, forcedOs }: AppProps) {
  if (isCapsule) {
    return <Capsule />;
  }
  if (isQa) {
    return <QaPanel />;
  }
  if (isLessComputer) {
    return <LessComputerPanel />;
  }
  if (isLessComputerGlow) {
    return <LessComputerGlow />;
  }

  const os = forcedOs ?? detectOS();
  // Windows 启动不应被权限探测阻塞首屏。
  const [gate, setGate] = useState<Gate>('ready');
  const [platformCaps, setPlatformCaps] = useState<PlatformCapabilities | null>(null);
  const [mobileQaOpen, setMobileQaOpen] = useState(false);
  useEffect(() => {
    applyAppTheme(readAppTheme());
    const syncTheme = (event: StorageEvent) => {
      if (event.key !== APP_THEME_KEY) return;
      applyAppTheme(readAppTheme());
    };
    window.addEventListener('storage', syncTheme);
    return () => window.removeEventListener('storage', syncTheme);
  }, []);

  useEffect(() => {
    if (!isTauri) return;
    void getPlatformCapabilities().then(setPlatformCaps);
  }, []);

  useEffect(() => {
    if (!isTauri || platformCaps?.platform !== 'android') return;
    let unlistenState: (() => void) | undefined;
    let unlistenDismiss: (() => void) | undefined;
    let cancelled = false;
    (async () => {
      try {
        const { listen } = await import('@tauri-apps/api/event');
        const stateHandle = await listen('qa:state', () => {
          setMobileQaOpen(true);
        });
        const dismissHandle = await listen('qa:dismiss', () => {
          setMobileQaOpen(false);
        });
        if (cancelled) {
          stateHandle();
          dismissHandle();
        } else {
          unlistenState = stateHandle;
          unlistenDismiss = dismissHandle;
        }
      } catch (error) {
        console.warn('[qa] mobile route listener setup failed', error);
      }
    })();
    return () => {
      cancelled = true;
      unlistenState?.();
      unlistenDismiss?.();
    };
  }, [platformCaps?.platform]);

  useEffect(() => {
    if (!mobileQaOpen || platformCaps?.platform !== 'android') return;
    window.history.pushState({ openlessQa: true }, '', window.location.href);
    const onPopState = () => {
      setMobileQaOpen(false);
      void qaWindowDismiss().catch(error => console.warn('[qa] mobile back dismiss failed', error));
    };
    window.addEventListener('popstate', onPopState);
    return () => {
      window.removeEventListener('popstate', onPopState);
    };
  }, [mobileQaOpen, platformCaps?.platform]);

  useEffect(() => {
    if (!isTauri) return;
    let cancelled = false;
    requestAnimationFrame(() => {
      if (cancelled) return;
      (async () => {
        // 尊重 prefs.startMinimized：开了静默启动就别在前端强 show 主窗口。否则
        // Rust 端 setup() 抑制掉的窗口，会被这条 useEffect 在 webview 加载完成后
        // 再通过 IPC 拉出来 —— issue #468 在 Rust 修复后用户仍能在 Win11 上复现
        // 的最后一条路径（Rust log 里看不到，因为走的是 plugin-window 的 IPC）。
        try {
          const prefs = await getSettings();
          if (prefs.startMinimized) return;
        } catch (err) {
          // 安全侧默认 = 不弹窗。Rust 端 get_settings 签名是
          // `pub fn get_settings(...) -> UserPreferences`（非 Result），所以
          // 该 catch 唯一会被触发的场景是 Tauri IPC 基础设施抖动（autostart 早期
          // __TAURI_INTERNALS__ 还没就绪）。旧逻辑 fall-through to show 会在用户
          // 开了静默启动时仍把主窗口弹出来 —— #468 复现路径。
          //
          // 此时 tray 已由 Rust 端 setup() 在 webview 加载前注册完成，是稳定的
          // 兜底入口；宁可让用户从 tray 手动唤起，也不要在抖动时强 show 一个白色
          // / 透明主窗口。首次安装的"prefs 不存在"场景不走这里 —— Rust 端会返回
          // 默认 UserPreferences。
          const detail = err instanceof Error ? err.message : String(err);
          console.warn('[startup] read startMinimized failed; staying hidden to avoid #468:', detail, err);
          return;
        }
        const { getCurrentWindow } = await import('@tauri-apps/api/window');
        if (cancelled) return;
        const currentWindow = getCurrentWindow();
        if (!(await currentWindow.isVisible())) {
          await currentWindow.show();
        }
      })().catch(error => console.warn('[startup] show main window failed', error));
    });
    return () => {
      cancelled = true;
    };
  }, [os]);

  useEffect(() => {
    if (!isTauri) return;
    let cancelled = false;

    void (async () => {
      const caps = await getPlatformCapabilities();
      if (cancelled) return;

      if (caps.platform === 'android') {
        const m = await checkMicrophonePermission();
        if (cancelled) return;
        // notDetermined is non-blocking on Android — show grant flow in-app instead
        // of trapping users on onboarding while JNI/runtime permission is pending.
        const blocked = m === 'denied' || m === 'restricted';
        setGate(blocked ? 'onboarding' : 'ready');
        return;
      }

      if (os === 'win') {
        // 超时保护：50 次 × 200ms = 10s。hotkey hook 永远 starting（被反作弊 / EDR
        // / UAC 拦）时不让 UI 死锁灰屏，过 10s 强 setGate('ready') 让用户进
        // Permissions 页看 hotkey_status.lastError 处理。详见 issue #163。
        const POLL_INTERVAL_MS = 200;
        const POLL_MAX_ATTEMPTS = 50;
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
        return;
      }

      const [a, m] = await Promise.all([
        checkAccessibilityPermission(),
        checkMicrophonePermission(),
      ]);
      if (cancelled) return;
      const aOk = a === 'granted' || a === 'notApplicable';
      const mOk = m === 'granted' || m === 'notApplicable';
      setGate(aOk && mOk ? 'ready' : 'onboarding');
    })().catch(error => {
      console.warn('[startup] permission gate failed', error);
      if (!cancelled) {
        setGate('ready');
      }
    });

    return () => {
      cancelled = true;
    };
  }, [os]);

  useEffect(() => {
    if (!isTauri || os !== 'win') return;
    const forwardKey = (event: KeyboardEvent) => {
      if (!isWindowHotkeyKeyboardCandidate(event)) return;
      void handleWindowHotkeyEvent(
        event.type as 'keydown' | 'keyup',
        event.key,
        event.code,
        event.repeat,
      ).catch(error => console.warn('[window-hotkey] forward failed', error));
    };
    const forwardMouse = (event: MouseEvent) => {
      const code = windowMouseHotkeyCode(event.button);
      if (!code) return;
      void handleWindowHotkeyEvent(
        event.type === 'mousedown' ? 'keydown' : 'keyup',
        code,
        code,
        false,
      ).catch(error => console.warn('[window-hotkey] mouse forward failed', error));
    };
    window.addEventListener('keydown', forwardKey, true);
    window.addEventListener('keyup', forwardKey, true);
    window.addEventListener('mousedown', forwardMouse, true);
    window.addEventListener('mouseup', forwardMouse, true);
    return () => {
      window.removeEventListener('keydown', forwardKey, true);
      window.removeEventListener('keyup', forwardKey, true);
      window.removeEventListener('mousedown', forwardMouse, true);
      window.removeEventListener('mouseup', forwardMouse, true);
    };
  }, [os]);

  return (
    <HotkeySettingsProvider>
      {platformCaps?.platform === 'android' && (
        <div style={{ display: mobileQaOpen ? 'block' : 'none', height: '100%' }}>
          <QaPanel
            embedded
            onRequestClose={() => {
              setMobileQaOpen(false);
              if (window.history.state?.openlessQa === true) {
                window.history.back();
              }
            }}
          />
        </div>
      )}
      {!mobileQaOpen && (gate === 'onboarding' ? (
        <Onboarding onComplete={() => setGate('ready')} />
      ) : (
        <FloatingShell os={os} />
      ))}
      {gate === 'ready' && platformCaps?.supportsAutoUpdate === true && <AutoUpdateGate />}
    </HotkeySettingsProvider>
  );
}
