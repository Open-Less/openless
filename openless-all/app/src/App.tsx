import { lazy, Suspense, useEffect, useState } from 'react';
import { Capsule } from './components/Capsule';
import { CapsuleTranscriptOverlay } from './components/LiveTranscriptPill';
import { CoreStartupScreen } from './components/CoreStartupScreen';
import { GlobalDownloadProgress } from './components/GlobalDownloadProgress';
import { detectOS, type OS } from './components/WindowChrome';
import {
  checkAccessibilityPermission,
  checkMicrophonePermission,
  getHotkeyStatus,
  getStartupSnapshot,
  getVoiceEditState,
  getSettings,
  getPlatformCapabilities,
  handleWindowHotkeyEvent,
  isTauri,
  qaWindowDismiss,
} from './lib/ipc';
import { cancelAndCloseVoiceEditSession } from './lib/ipc/voice-edit-session';
import type { PlatformCapabilities } from './lib/types';
import { isWindowHotkeyKeyboardCandidate, windowMouseHotkeyCode } from './lib/windowHotkeyFallback';
import { HotkeySettingsProvider } from './state/HotkeySettingsContext';

// Lazy-load pages per WebView by window purpose to cut resident memory. The capsule
// gives instant recording feedback — small and first-frame-latency sensitive — so it
// stays a direct import.
const AutoUpdateGate = lazy(() =>
  import('./components/AutoUpdateGate').then((m) => ({ default: m.AutoUpdateGate })),
);
const FloatingShell = lazy(() =>
  import('./components/FloatingShell').then((m) => ({ default: m.FloatingShell })),
);
const Onboarding = lazy(() =>
  import('./components/Onboarding').then((m) => ({ default: m.Onboarding })),
);
const QaPanel = lazy(() => import('./pages/QaPanel').then((m) => ({ default: m.QaPanel })));
const SelectionVoiceIntentPicker = lazy(() =>
  import('./pages/SelectionVoiceIntentPicker').then((m) => ({
    default: m.SelectionVoiceIntentPicker,
  })),
);
const VoiceEditPanel = lazy(() =>
  import('./pages/VoiceEditPanel').then((m) => ({ default: m.VoiceEditPanel })),
);
// Tauri's Less Computer panel targets macOS and Windows; Linux gets the native egui
// UI instead. TAURI_ENV_PLATFORM is a compile-time literal, so platforms that don't
// run this WebView can drop the import, keeping the panel chunk out of mobile builds.
// Plain-browser vite (preview/styling) lacks the variable → stays loadable.
const TAURI_BUILD_PLATFORM: string | undefined = import.meta.env.TAURI_ENV_PLATFORM;
const LESS_COMPUTER_BUNDLED =
  !TAURI_BUILD_PLATFORM || TAURI_BUILD_PLATFORM === 'darwin' || TAURI_BUILD_PLATFORM === 'windows';
const LessComputerPanel = LESS_COMPUTER_BUNDLED
  ? lazy(() => import('./pages/LessComputerPanel').then((m) => ({ default: m.LessComputerPanel })))
  : null;
const LessComputerGlow = LESS_COMPUTER_BUNDLED
  ? lazy(() => import('./pages/LessComputerGlow').then((m) => ({ default: m.LessComputerGlow })))
  : null;

interface AppProps {
  isCapsule: boolean;
  isCapsuleRail: boolean;
  isQa: boolean;
  isSelectionVoiceIntent: boolean;
  isVoiceEdit: boolean;
  isLessComputer: boolean;
  isLessComputerGlow: boolean;
  forcedOs?: OS | null;
}

type Gate = 'checking' | 'incompatible' | 'onboarding' | 'ready';
const ANDROID_SETUP_WIZARD_COMPLETE_KEY = 'openless.androidSetupWizardComplete';

/**
 * All Tauri webviews share the same fail-closed startup boundary. Capsule, QA, preview,
 * and Less Computer also call business IPC, so they can't skip the 2.0 handshake just
 * for not being the main window. requireBackendReady reuses one Promise internally, so
 * later main-window reads never trigger a second startup request.
 */
export function App(props: AppProps) {
  const [ready, setReady] = useState(!isTauri);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!isTauri) return;
    void getStartupSnapshot()
      .then(() => setReady(true))
      .catch((reason) => {
        const detail = reason instanceof Error ? reason.message : String(reason);
        console.error('[startup] backend contract handshake failed', reason);
        setError(detail);
      });
  }, []);

  if (error) {
    return (
      <CoreStartupScreen error={error} compact={props.isCapsule || props.isLessComputerGlow} />
    );
  }
  if (!ready) {
    return <CoreStartupScreen compact={props.isCapsule || props.isLessComputerGlow} />;
  }
  return <ReadyApp {...props} />;
}

function ReadyApp({
  isCapsule,
  isCapsuleRail,
  isQa,
  isSelectionVoiceIntent,
  isVoiceEdit,
  isLessComputer,
  isLessComputerGlow,
  forcedOs,
}: AppProps) {
  if (isCapsuleRail) {
    return <CapsuleTranscriptOverlay />;
  }
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
  if (isSelectionVoiceIntent) {
    return (
      <Suspense fallback={null}>
        <SelectionVoiceIntentPicker />
      </Suspense>
    );
  }
  if (isVoiceEdit) {
    return (
      <Suspense fallback={null}>
        <VoiceEditPanel />
      </Suspense>
    );
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
  // Windows startup must not block the first screen on permission probes.
  const [gate, setGate] = useState<Gate>(isTauri ? 'checking' : 'ready');
  const [startupError, setStartupError] = useState<string | null>(null);
  const [platformCaps, setPlatformCaps] = useState<PlatformCapabilities | null>(null);
  const [mobileQaOpen, setMobileQaOpen] = useState(false);
  const [mobileVoiceEditOpen, setMobileVoiceEditOpen] = useState(false);
  const [mobileVoiceEditCloseError, setMobileVoiceEditCloseError] = useState('');
  const completeOnboarding = () => {
    if (platformCaps?.platform === 'android') {
      localStorage.setItem(ANDROID_SETUP_WIZARD_COMPLETE_KEY, '1');
    }
    setGate('ready');
  };
  useEffect(() => {
    if (!isTauri) return;
    void getStartupSnapshot()
      .then(() => getPlatformCapabilities())
      .then(setPlatformCaps)
      .catch((error) => {
        const detail = error instanceof Error ? error.message : String(error);
        console.error('[startup] backend contract handshake failed', error);
        setStartupError(detail);
        setGate('incompatible');
      });
  }, []);

  useEffect(() => {
    if (!isTauri || platformCaps?.platform !== 'android') return;
    let unlistenState: (() => void) | undefined;
    let unlistenDismiss: (() => void) | undefined;
    let unlistenVoiceEditShow: (() => void) | undefined;
    let unlistenVoiceEditDismiss: (() => void) | undefined;
    let cancelled = false;
    (async () => {
      try {
        const { listen } = await import('@tauri-apps/api/event');
        const stateHandle = await listen('qa:state', async () => {
          console.info('[qa] android qa:state received; opening embedded panel');
          setMobileVoiceEditCloseError('');
          try {
            await cancelAndCloseVoiceEditSession();
            if (cancelled) return;
            setMobileVoiceEditOpen(false);
            setMobileQaOpen(true);
          } catch (error) {
            if (cancelled) return;
            setMobileVoiceEditCloseError(error instanceof Error ? error.message : String(error));
            setMobileVoiceEditOpen(true);
          }
        });
        const dismissHandle = await listen('qa:dismiss', () => {
          console.info('[qa] android qa:dismiss received; closing embedded panel');
          setMobileQaOpen(false);
        });
        const voiceEditShowHandle = await listen('voice-edit:show', () => {
          console.info('[voice-edit] android show requested; opening embedded panel');
          setMobileVoiceEditCloseError('');
          setMobileQaOpen(false);
          setMobileVoiceEditOpen(true);
        });
        const voiceEditDismissHandle = await listen('voice-edit:dismiss', () => {
          console.info('[voice-edit] android dismiss requested; closing embedded panel');
          setMobileVoiceEditOpen(false);
        });
        if (cancelled) {
          stateHandle();
          dismissHandle();
          voiceEditShowHandle();
          voiceEditDismissHandle();
        } else {
          unlistenState = stateHandle;
          unlistenDismiss = dismissHandle;
          unlistenVoiceEditShow = voiceEditShowHandle;
          unlistenVoiceEditDismiss = voiceEditDismissHandle;
          const current = await getVoiceEditState();
          if (
            !cancelled &&
            current &&
            current.phase !== 'completed' &&
            current.phase !== 'cancelled'
          ) {
            setMobileQaOpen(false);
            setMobileVoiceEditOpen(true);
          }
        }
      } catch (error) {
        console.warn('[qa] mobile route listener setup failed', error);
      }
    })();
    return () => {
      cancelled = true;
      unlistenState?.();
      unlistenDismiss?.();
      unlistenVoiceEditShow?.();
      unlistenVoiceEditDismiss?.();
    };
  }, [platformCaps?.platform]);

  useEffect(() => {
    if (!mobileQaOpen || platformCaps?.platform !== 'android') return;
    window.history.pushState({ openlessQa: true }, '', window.location.href);
    const onPopState = () => {
      setMobileQaOpen(false);
      void qaWindowDismiss().catch((error) =>
        console.warn('[qa] mobile back dismiss failed', error),
      );
    };
    window.addEventListener('popstate', onPopState);
    return () => {
      window.removeEventListener('popstate', onPopState);
    };
  }, [mobileQaOpen, platformCaps?.platform]);

  useEffect(() => {
    if (!mobileVoiceEditOpen || platformCaps?.platform !== 'android') return;
    window.history.pushState({ openlessVoiceEdit: true }, '', window.location.href);
  }, [mobileVoiceEditOpen, platformCaps?.platform]);

  useEffect(() => {
    if (!isTauri || !platformCaps) return;
    let cancelled = false;
    requestAnimationFrame(() => {
      if (cancelled) return;
      (async () => {
        // Respect prefs.startMinimized: with silent start, don't force-show the main
        // window from the frontend. Otherwise this useEffect pulls the window that
        // Rust setup() suppressed back out via IPC after webview load — the last
        // remaining #468 repro path on Win11 after the Rust fix (invisible in Rust
        // logs because it goes through plugin-window IPC).
        try {
          const prefs = await getSettings();
          if (prefs.startMinimized) return;
        } catch (err) {
          // Safe default = stay hidden. Rust's get_settings returns UserPreferences
          // (not a Result), so this catch only fires on Tauri IPC infra flakiness
          // (__TAURI_INTERNALS__ not ready early in autostart). The old fall-through
          // to show would still pop the main window with silent start on — the #468
          // repro path.
          //
          // By now the tray is registered by Rust setup() before webview load and is a
          // stable fallback; better to have the user raise the window from the tray
          // than force-show a white/transparent main window mid-flake. First-install
          // "no prefs" doesn't reach here — Rust returns default UserPreferences.
          const detail = err instanceof Error ? err.message : String(err);
          console.warn(
            '[startup] read startMinimized failed; staying hidden to avoid #468:',
            detail,
            err,
          );
          return;
        }
        const { getCurrentWindow } = await import('@tauri-apps/api/window');
        if (cancelled) return;
        const currentWindow = getCurrentWindow();
        if (!(await currentWindow.isVisible())) {
          await currentWindow.show();
        }
      })().catch((error) => console.warn('[startup] show main window failed', error));
    });
    return () => {
      cancelled = true;
    };
  }, [os, platformCaps]);

  useEffect(() => {
    if (!isTauri || !platformCaps) return;
    let cancelled = false;

    void (async () => {
      const caps = platformCaps;

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
        // Timeout guard: 50 × 200ms = 10s. If the hotkey hook stays "starting" forever
        // (blocked by anti-cheat / EDR / UAC), don't deadlock the UI on a gray screen;
        // after 10s force setGate('ready') so the user can reach the Permissions page
        // and check hotkey_status.lastError. See issue #163.
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
          await new Promise((resolve) => window.setTimeout(resolve, POLL_INTERVAL_MS));
        }
        if (!cancelled) {
          console.warn(
            `[startup] hotkey gate timed out after ${POLL_MAX_ATTEMPTS * POLL_INTERVAL_MS}ms; forcing ready so user can reach Permissions page`,
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
      // noDevice (no mic present) isn't a permission issue: don't trap the user on
      // onboarding; let them in and show the clear "no microphone detected" notice on
      // the permissions page. See issue #779.
      const mOk = m === 'granted' || m === 'notApplicable' || m === 'noDevice';
      setGate(aOk && mOk ? 'ready' : 'onboarding');
    })().catch((error) => {
      console.warn('[startup] permission gate failed', error);
      if (!cancelled) {
        setGate('ready');
      }
    });

    return () => {
      cancelled = true;
    };
  }, [os, platformCaps]);

  useEffect(() => {
    if (!isTauri || os !== 'win') return;
    const forwardKey = (event: KeyboardEvent) => {
      if (!isWindowHotkeyKeyboardCandidate(event)) return;
      void handleWindowHotkeyEvent(
        event.type as 'keydown' | 'keyup',
        event.key,
        event.code,
        event.repeat,
      ).catch((error) => console.warn('[window-hotkey] forward failed', error));
    };
    const forwardMouse = (event: MouseEvent) => {
      const code = windowMouseHotkeyCode(event.button);
      if (!code) return;
      void handleWindowHotkeyEvent(
        event.type === 'mousedown' ? 'keydown' : 'keyup',
        code,
        code,
        false,
      ).catch((error) => console.warn('[window-hotkey] mouse forward failed', error));
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
    return <CoreStartupScreen />;
  }
  if (gate === 'incompatible') {
    return <CoreStartupScreen error={startupError ?? 'Core startup failed'} />;
  }

  return (
    <Suspense fallback={null}>
      <HotkeySettingsProvider>
        {/* Global download progress overlay: always mounted across main-window pages
            (listens to events itself, decoupled from pages). */}
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
        {platformCaps?.platform === 'android' && (
          <div style={{ display: mobileVoiceEditOpen ? 'block' : 'none', height: '100%' }}>
            <VoiceEditPanel
              embedded
              active={mobileVoiceEditOpen}
              closeError={mobileVoiceEditCloseError}
              onRequestClose={() => {
                setMobileVoiceEditOpen(false);
                if (window.history.state?.openlessVoiceEdit === true) {
                  window.history.back();
                }
              }}
            />
          </div>
        )}
        {!mobileQaOpen &&
          !mobileVoiceEditOpen &&
          (gate === 'onboarding' ? (
            <Onboarding onComplete={completeOnboarding} />
          ) : (
            <FloatingShell os={os} />
          ))}
        {gate === 'ready' && platformCaps?.supportsAutoUpdate === true && <AutoUpdateGate />}
      </HotkeySettingsProvider>
    </Suspense>
  );
}
