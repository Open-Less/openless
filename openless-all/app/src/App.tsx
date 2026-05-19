import { useEffect, useState } from 'react';
import { AutoUpdateGate } from './components/AutoUpdateGate';
import { Capsule } from './components/Capsule';
import { FloatingShell } from './components/FloatingShell';
import {
  ONBOARDING_COMPLETE_KEY,
  Onboarding,
  REQUIRED_ONBOARDING_VERSION,
  type OnboardingCompleteOptions,
} from './components/Onboarding';
import { detectOS } from './components/WindowChrome';
import {
  checkAccessibilityPermission,
  checkMicrophonePermission,
  getHotkeyStatus,
  getSettings,
  handleWindowHotkeyEvent,
  isTauri,
} from './lib/ipc';
import type { PermissionStatus, UserPreferences } from './lib/types';
import {
  isWindowHotkeyKeyboardCandidate,
  windowMouseHotkeyCode,
} from './lib/windowHotkeyFallback';
import { QaPanel } from './pages/QaPanel';
import type { SettingsSectionId } from './pages/Settings';
import { HotkeySettingsProvider } from './state/HotkeySettingsContext';
import { PROVIDER_SETUP_PROMPT_DEFERRED_KEY } from './lib/providerSetup';

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
  const [gate, setGate] = useState<Gate>('checking');
  const [postOnboardingSettingsSection, setPostOnboardingSettingsSection] = useState<SettingsSectionId | undefined>();

  useEffect(() => {
    if (!isTauri) return;
    if (os === 'win' && gate === 'checking') return;
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
          if (prefs.startMinimized && gate !== 'onboarding') return;
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
  }, [gate, os]);

  useEffect(() => {
    let cancelled = false;

    const decideGate = async () => {
      if (isTauri && os === 'win') {
        await waitForWindowsHotkeyGate(() => cancelled);
        if (cancelled) return;
      }
      const nextGate = await resolveStartupGate();
      if (!cancelled) {
        setGate(nextGate);
      }
    };

    void decideGate().catch(error => {
      console.warn('[startup] startup gate failed, showing onboarding fallback', error);
      if (!cancelled) {
        setGate('onboarding');
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

  if (gate === 'checking') {
    return <StartupShell />;
  }
  const startOnboarding = () => {
    try {
      window.localStorage.removeItem(ONBOARDING_COMPLETE_KEY);
    } catch {
      // Ignore unavailable localStorage; persisted preferences are the source of truth.
    }
    setPostOnboardingSettingsSection(undefined);
    setGate('onboarding');
  };
  const finishOnboarding = (options?: OnboardingCompleteOptions) => {
    const nextSection = options?.openSettingsSection;
    setPostOnboardingSettingsSection(nextSection);
    if (nextSection) {
      try {
        window.sessionStorage.setItem(PROVIDER_SETUP_PROMPT_DEFERRED_KEY, '1');
      } catch {
        // Avoid covering the settings jump with the provider setup prompt when sessionStorage is unavailable.
      }
    }
    setGate('ready');
  };
  return (
    <HotkeySettingsProvider>
      {gate === 'onboarding'
        ? <Onboarding onComplete={finishOnboarding} />
        : (
          <FloatingShell
            initialSettings={postOnboardingSettingsSection !== undefined}
            initialSettingsSection={postOnboardingSettingsSection}
            onStartOnboarding={startOnboarding}
          />
        )}
      {gate === 'ready' && <AutoUpdateGate />}
    </HotkeySettingsProvider>
  );
}

async function waitForWindowsHotkeyGate(isCancelled: () => boolean) {
  const POLL_INTERVAL_MS = 200;
  const POLL_MAX_ATTEMPTS = 50;
  let attempts = 0;
  while (!isCancelled() && attempts < POLL_MAX_ATTEMPTS) {
    attempts += 1;
    const status = await getHotkeyStatus();
    if (status.state !== 'starting') return;
    await new Promise(resolve => window.setTimeout(resolve, POLL_INTERVAL_MS));
  }
  if (!isCancelled()) {
    console.warn(
      `[startup] hotkey gate timed out after ${POLL_MAX_ATTEMPTS * POLL_INTERVAL_MS}ms; continuing to startup onboarding decision`
    );
  }
}

async function resolveStartupGate(): Promise<Gate> {
  const prefs = await getSettings().catch(() => null);
  if (!onboardingMarkedComplete(prefs)) {
    return 'onboarding';
  }

  const [accessibility, microphone] = await Promise.allSettled([
    checkAccessibilityPermission(),
    checkMicrophonePermission(),
  ]);

  const aOk = accessibility.status === 'fulfilled'
    ? permissionReady(accessibility.value)
    : false;
  const mOk = microphone.status === 'fulfilled'
    ? permissionReady(microphone.value)
    : false;
  if (!aOk || !mOk) return 'onboarding';
  return 'ready';
}

function permissionReady(status: PermissionStatus) {
  return status === 'granted' || status === 'notApplicable';
}

function onboardingMarkedComplete(prefs: Pick<UserPreferences, 'onboardingVersion'> | null) {
  if (prefs) {
    return prefs.onboardingVersion >= REQUIRED_ONBOARDING_VERSION;
  }
  try {
    return window.localStorage.getItem(ONBOARDING_COMPLETE_KEY) === '1';
  } catch {
    return false;
  }
}

function StartupShell() {
  // 用透明背景：main window 是 transparent + macOSPrivateApi（NSVisualEffectView 磨砂）。
  // 之前用 linear-gradient(rgba(245,245,247,0.96)...) 会盖过 macOS vibrancy，启动时
  // 长时间在 'checking' phase（凭据迁移 / 权限 probe 慢）会让窗口看起来「左侧白屏 +
  // 右侧磨砂」割裂。现在背景全透明，让磨砂统一展开，提示文字 + icon 用一个轻量
  // pill 卡片承载，跟 capsule 视觉一致。
  return (
    <div
      style={{
        minHeight: '100vh',
        display: 'grid',
        placeItems: 'center',
        background: 'transparent',
        color: 'var(--ol-ink-3)',
        fontFamily: 'var(--ol-font-sans)',
      }}
    >
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 10,
          fontSize: 13,
          fontWeight: 500,
          padding: '10px 16px',
          borderRadius: 999,
          background: 'rgba(255, 255, 255, 0.55)',
          backdropFilter: 'blur(20px) saturate(180%)',
          WebkitBackdropFilter: 'blur(20px) saturate(180%)',
          border: '0.5px solid rgba(0, 0, 0, 0.06)',
          boxShadow: '0 4px 14px -6px rgba(0, 0, 0, 0.18), 0 0 0 0.5px rgba(0,0,0,0.04)',
        }}
      >
        <img src="AppIcon.png" alt="" style={{ width: 18, height: 18, borderRadius: 4 }} />
        <span>OpenLess 正在启动</span>
      </div>
    </div>
  );
}
