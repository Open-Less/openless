import { lazy, Suspense, useEffect, useState } from 'react';
import { Capsule } from './components/Capsule';
import { GlobalDownloadProgress } from './components/GlobalDownloadProgress';
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
import { HotkeySettingsProvider } from './state/HotkeySettingsContext';

// 各窗口/重页面懒加载,让每个 webview 只下载并解析自己用到的那部分代码。原本所有窗口
// (主设置 / 胶囊 / QA / Less Computer / glow)共用一个打包产物,导致 5 个常驻 WebKit
// 进程都把整套设置 UI(FloatingShell + Style/Marketplace/LocalAsr…)和聊天面板加载进来,
// 常驻内存离谱。拆开后胶囊/glow 这类轻窗口不再加载设置/聊天代码。胶囊保持 eager:
// 它是听写实时反馈、对首帧延迟敏感,且体积很小。
const AutoUpdateGate = lazy(() =>
  import('./components/AutoUpdateGate').then(m => ({ default: m.AutoUpdateGate })),
);
const FloatingShell = lazy(() =>
  import('./components/FloatingShell').then(m => ({ default: m.FloatingShell })),
);
const Onboarding = lazy(() =>
  import('./components/Onboarding').then(m => ({ default: m.Onboarding })),
);
const QaPanel = lazy(() => import('./pages/QaPanel').then(m => ({ default: m.QaPanel })));
const SelectionPolishPreview = lazy(() => import('./pages/SelectionPolishPreview').then(m => ({ default: m.SelectionPolishPreview })));
const SelectionVoiceIntentPicker = lazy(() => import('./pages/SelectionVoiceIntentPicker').then(m => ({ default: m.SelectionVoiceIntentPicker })));
// Less Computer 仅 macOS 开放（后端只在 macOS 注册热键/创建窗口）。Tauri 构建时
// TAURI_ENV_PLATFORM 是编译期字面量：非 macOS 平台下面两个三元的 import() 分支
// 被常量折叠 + DCE 整个裁掉，面板 chunk 不进打包产物（门控 = 不打包）。
// 纯浏览器 vite 环境（预览/调样式）没有该变量 → 保持可加载。
const TAURI_BUILD_PLATFORM: string | undefined = import.meta.env.TAURI_ENV_PLATFORM;
const LESS_COMPUTER_BUNDLED = !TAURI_BUILD_PLATFORM || TAURI_BUILD_PLATFORM === 'darwin';
const LessComputerPanel = LESS_COMPUTER_BUNDLED
  ? lazy(() => import('./pages/LessComputerPanel').then(m => ({ default: m.LessComputerPanel })))
  : null;
const LessComputerGlow = LESS_COMPUTER_BUNDLED
  ? lazy(() => import('./pages/LessComputerGlow').then(m => ({ default: m.LessComputerGlow })))
  : null;

interface AppProps {
  isCapsule: boolean;
  isQa: boolean;
  isSelectionPolishPreview: boolean;
  isSelectionVoiceIntent: boolean;
  isLessComputer: boolean;
  isLessComputerGlow: boolean;
  forcedOs?: OS | null;
}

type Gate = 'onboarding' | 'ready';
const ANDROID_SETUP_WIZARD_COMPLETE_KEY = 'openless.androidSetupWizardComplete';

export function App({ isCapsule, isQa, isSelectionPolishPreview, isSelectionVoiceIntent, isLessComputer, isLessComputerGlow, forcedOs }: AppProps) {
  if (isCapsule) {
    return <Capsule os={forcedOs} />;
  }
  if (isQa) {
    return (
      <Suspense fallback={null}>
        <QaPanel />
      </Suspense>
    );
  }
  if (isSelectionPolishPreview) {
    return <Suspense fallback={null}><SelectionPolishPreview /></Suspense>;
  }
  if (isSelectionVoiceIntent) {
    return <Suspense fallback={null}><SelectionVoiceIntentPicker /></Suspense>;
  }
  if (isLessComputer) {
    return LessComputerPanel ? (
      <Suspense fallback={null}>
        <LessComputerPanel />
      </Suspense>
    ) : null;
  }
  if (isLessComputerGlow) {
    return LessComputerGlow ? (
      <Suspense fallback={null}>
        <LessComputerGlow />
      </Suspense>
    ) : null;
  }

  const os = forcedOs ?? detectOS();
  // Windows 启动不应被权限探测阻塞首屏。
  const [gate, setGate] = useState<Gate>('ready');
  const [platformCaps, setPlatformCaps] = useState<PlatformCapabilities | null>(null);
  const [mobileQaOpen, setMobileQaOpen] = useState(false);
  const completeOnboarding = () => {
    if (platformCaps?.platform === 'android') {
      localStorage.setItem(ANDROID_SETUP_WIZARD_COMPLETE_KEY, '1');
    }
    setGate('ready');
  };
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
          console.info('[qa] android qa:state received; opening embedded panel');
          setMobileQaOpen(true);
        });
        const dismissHandle = await listen('qa:dismiss', () => {
          console.info('[qa] android qa:dismiss received; closing embedded panel');
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
        if (localStorage.getItem(ANDROID_SETUP_WIZARD_COMPLETE_KEY) !== '1') {
          setGate('onboarding');
          return;
        }
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
      // noDevice（当前没有麦克风）不是权限问题：不卡 onboarding，
      // 让用户进应用后在权限页看到“未检测到麦克风”的明确提示。见 issue #779。
      const mOk = m === 'granted' || m === 'notApplicable' || m === 'noDevice';
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
    <Suspense fallback={null}>
      <HotkeySettingsProvider>
        {/* 全局下载进度浮层：主窗口所有页面常驻（自身监听事件，与页面解耦）。 */}
        <GlobalDownloadProgress />
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
          <Onboarding onComplete={completeOnboarding} />
        ) : (
          <FloatingShell os={os} />
        ))}
        {gate === 'ready' && platformCaps?.supportsAutoUpdate === true && <AutoUpdateGate />}
      </HotkeySettingsProvider>
    </Suspense>
  );
}
