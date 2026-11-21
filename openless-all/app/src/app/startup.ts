import { startTransition, useEffect, useRef, useState } from 'react';
import { getCredentials } from '../api/providers';
import { getHotkeyStatus } from '../api/hotkeys';
import { checkAccessibilityPermission, checkMicrophonePermission } from '../api/permissions';
import { getSettings } from '../api/preferences';
import {
  getDesktopOnboardingState,
  needsDesktopGuide,
  newGuideProgress,
  permissionGate,
  readGuideProgress,
  REOPEN_ONBOARDING_EVENT,
  saveGuideProgress,
  setDesktopOnboardingFinished,
} from '../lib/desktopOnboarding';
import { withStartupDeadline } from '../lib/startupProbe';
import { isTauriHost, type OS } from '../runtime/environment';
import { getPlatformCapabilities, type PlatformCapabilities } from '../runtime/platform';

type StartupGate = 'checking' | 'onboarding' | 'ready' | 'failed';
const ANDROID_COMPLETION_KEY = 'openless.androidSetupWizardComplete';
const WINDOWS_HOTKEY_POLL_DELAY_MS = 200;
const WINDOWS_HOTKEY_POLL_ATTEMPTS = 50;

import { loadMainShell } from './destinations';

async function selectNativeDestination(os: OS, caps: PlatformCapabilities, signal: AbortSignal) {
  if (caps.platform === 'android') {
    if (localStorage.getItem(ANDROID_COMPLETION_KEY) !== '1') return 'onboarding';
    const permission = await withStartupDeadline(checkMicrophonePermission(), signal);
    // notDetermined still has an in-app grant path while JNI is becoming available.
    return permission === 'denied' || permission === 'restricted' ? 'onboarding' : 'ready';
  }

  const progress = readGuideProgress();
  const [credentials, persisted] = await Promise.all([
    withStartupDeadline(getCredentials(), signal),
    withStartupDeadline(getDesktopOnboardingState(), signal).catch((error) => {
      if (!signal.aborted) console.warn('[startup] persisted onboarding state unavailable', error);
      return { finished: false };
    }),
  ]);
  if (signal.aborted) return;
  if (needsDesktopGuide(progress, credentials, persisted.finished)) return 'onboarding';

  if (os === 'win') {
    for (let attempt = 0; !signal.aborted && attempt < WINDOWS_HOTKEY_POLL_ATTEMPTS; attempt++) {
      const status = await withStartupDeadline(getHotkeyStatus(), signal);
      if (signal.aborted) return;
      if (status.state === 'failed') {
        saveGuideProgress({
          ...(progress ?? newGuideProgress()),
          step: 'shortcut',
          finished: false,
        });
        return 'onboarding';
      }
      if (status.state !== 'starting') return 'ready';
      await new Promise((resolve) => window.setTimeout(resolve, WINDOWS_HOTKEY_POLL_DELAY_MS));
    }
    if (!signal.aborted)
      console.warn('[startup] hotkey still starting; opening the settings shell');
    return 'ready';
  }

  const [accessibility, microphone] = await withStartupDeadline(
    Promise.all([checkAccessibilityPermission(), checkMicrophonePermission()]),
    signal,
  );
  if (signal.aborted) return;
  const next = permissionGate({
    os,
    persistedFinished: persisted.finished,
    accessibility,
    microphone,
  });
  if (next.gate !== 'ready')
    saveGuideProgress({ ...(progress ?? newGuideProgress()), step: next.step, finished: false });
  return next.gate;
}

function useNativeVisibility(caps: PlatformCapabilities | null) {
  useEffect(() => {
    if (!isTauriHost() || !caps) return;
    let active = true;
    const frame = requestAnimationFrame(() => {
      void (async () => {
        // A failed read must not force-show the application during silent start.
        const prefs = await getSettings().catch((error: unknown) => {
          console.warn('[startup] failed to check silent start; keeping the window hidden', error);
          return null;
        });
        if (!active || !prefs || prefs.startMinimized) return;
        const { getCurrentWindow } = await import('@tauri-apps/api/window');
        if (!active) return;
        const window = getCurrentWindow();
        if (!(await window.isVisible()) && active) await window.show();
      })().catch((error) => console.warn('[startup] show main window failed', error));
    });
    return () => {
      active = false;
      cancelAnimationFrame(frame);
    };
  }, [caps]);
}

export function useMainStartup(os: OS, previewOnboarding: boolean, reducedMotion: boolean) {
  const tauri = isTauriHost();
  const [gate, setGate] = useState<StartupGate>(
    tauri ? 'checking' : previewOnboarding ? 'onboarding' : 'ready',
  );
  const [caps, setCaps] = useState<PlatformCapabilities | null>(null);
  const [error, setError] = useState<string>();
  const [onboardingHandoff, setOnboardingHandoff] = useState(false);
  const destinationGeneration = useRef(0);

  useEffect(() => {
    if (!tauri) return;
    let active = true;
    void getPlatformCapabilities()
      .then((next) => {
        if (active) setCaps(next);
      })
      .catch((reason: unknown) => {
        if (!active) return;
        console.error('[startup] platform capability handshake failed', reason);
        setError(reason instanceof Error ? reason.message : String(reason));
        setGate('failed');
      });
    return () => {
      active = false;
    };
  }, [tauri]);

  useNativeVisibility(caps);
  useEffect(() => {
    if (!tauri || !caps || destinationGeneration.current !== 0) return;
    const probe = new AbortController();
    const current = () => !probe.signal.aborted && destinationGeneration.current === 0;
    void selectNativeDestination(os, caps, probe.signal)
      .then((next) => {
        if (next && current()) setGate(next);
      })
      .catch((reason: unknown) => {
        if (!current()) return;
        console.warn('[startup] permission gate failed', reason);
        setGate('ready');
      });
    return () => probe.abort();
  }, [tauri, caps, os]);

  useEffect(() => {
    const reopen = () => {
      destinationGeneration.current++;
      if (tauri) {
        saveGuideProgress(newGuideProgress());
        void setDesktopOnboardingFinished(false).catch((reason) =>
          console.warn('[onboarding] clear persisted completion failed', reason),
        );
      }
      setOnboardingHandoff(false);
      setGate('onboarding');
    };
    window.addEventListener(REOPEN_ONBOARDING_EVENT, reopen);
    return () => window.removeEventListener(REOPEN_ONBOARDING_EVENT, reopen);
  }, [tauri]);

  useEffect(() => {
    if (!onboardingHandoff) return;
    const timer = window.setTimeout(() => setOnboardingHandoff(false), reducedMotion ? 0 : 280);
    return () => window.clearTimeout(timer);
  }, [onboardingHandoff, reducedMotion]);

  const completeOnboarding = async () => {
    const generation = ++destinationGeneration.current;
    if (caps?.platform === 'android') {
      localStorage.setItem(ANDROID_COMPLETION_KEY, '1');
    } else {
      await loadMainShell();
    }
    if (generation !== destinationGeneration.current) return;
    startTransition(() => {
      setOnboardingHandoff(caps?.platform !== 'android' && !reducedMotion);
      setGate('ready');
    });
  };

  return { gate, caps, error, onboardingHandoff, completeOnboarding };
}
