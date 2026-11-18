import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { CredentialsStatus, PermissionStatus } from '../../api/contracts';
import {
  deferPostGuidePrompts,
  DESKTOP_ONBOARDING_KEY,
  guideSteps,
  newGuideProgress,
  readGuideProgress,
  saveGuideProgress,
  setDesktopOnboardingFinished,
  type GuideAnswer,
  type GuideProgress,
  type GuideStep,
} from '../../api/onboarding';
import {
  checkAccessibilityPermission,
  checkMicrophonePermission,
  openSystemSettings,
  requestAccessibilityPermission,
  requestMicrophonePermission,
} from '../../api/permissions';
import { getCredentials } from '../../api/providers';
import { useHotkeySettings } from '../../data/preferences';
import { useProviderForm } from '../providers/ProviderForm';
import { getRuntimeOS, isPreviewHost, isTauriHost } from '../../runtime/environment';

export const granted = (status: PermissionStatus | null) =>
  status === 'granted' || status === 'notApplicable';

export function useDesktopGuide({ onComplete }: { onComplete: () => void | Promise<void> }) {
  const { t, i18n } = useTranslation();
  const preview = isPreviewHost();
  const os = getRuntimeOS();
  const {
    prefs,
    loading,
    error: preferencesError,
    refresh: refreshPreferences,
    updatePrefs,
  } = useHotkeySettings();
  const [progress, setProgress] = useState<GuideProgress>(() =>
    preview ? newGuideProgress() : (readGuideProgress() ?? newGuideProgress()),
  );
  const [name, setName] = useState(progress.name);
  const [microphone, setMicrophone] = useState<PermissionStatus | null>(
    preview ? 'notDetermined' : null,
  );
  const [accessibility, setAccessibility] = useState<PermissionStatus | null>(
    preview ? 'notDetermined' : null,
  );
  const [credentials, setCredentials] = useState<CredentialsStatus | null>(null);
  const [editing, setEditing] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [storageError, setStorageError] = useState(false);
  const pending = useRef(false);
  const alive = useRef(true);
  const permissionRequest = useRef(0);
  const form = useProviderForm();
  const stageRef = useRef<HTMLDivElement>(null);
  const multimodal =
    prefs?.multimodalPipelineEnabled === true && prefs.pipelineMode === 'multimodal';
  const steps = guideSteps(os === 'mac', multimodal);
  const step = steps.includes(progress.step) ? progress.step : 'recognition';
  const index = steps.indexOf(step);
  const currentMode = multimodal ? 'multimodal' : 'traditional';

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      permissionRequest.current++;
    };
  }, []);

  const refreshPermissions = useCallback(async () => {
    if (preview) return;
    const request = ++permissionRequest.current;
    const results = await Promise.allSettled([
      checkMicrophonePermission(),
      os === 'mac' ? checkAccessibilityPermission() : Promise.resolve('notApplicable' as const),
    ]);
    if (!alive.current || request !== permissionRequest.current) return;
    if (results[0].status === 'fulfilled') setMicrophone(results[0].value);
    if (results[1].status === 'fulfilled') setAccessibility(results[1].value);
    if (results.some((result) => result.status === 'rejected'))
      setError(t('desktopGuide.saveError'));
  }, [preview, os, t]);

  useEffect(() => {
    void refreshPermissions();
    let cancelled = false;
    void getCredentials()
      .then((value) => {
        if (!cancelled) setCredentials(value);
      })
      .catch(() => {});
    const onFocus = () => {
      void refreshPermissions();
    };
    const onVisible = () => {
      if (document.visibilityState === 'visible') onFocus();
    };
    window.addEventListener('focus', onFocus);
    document.addEventListener('visibilitychange', onVisible);
    return () => {
      cancelled = true;
      window.removeEventListener('focus', onFocus);
      document.removeEventListener('visibilitychange', onVisible);
    };
  }, [refreshPermissions]);

  const persist = (next: GuideProgress) => {
    setProgress(next);
    if (!preview) setStorageError(!saveGuideProgress(next));
  };
  const run = async (action: () => void | Promise<void>) => {
    if (pending.current) return;
    pending.current = true;
    setBusy(true);
    setError('');
    try {
      const saved = await form.finish(action);
      if (!saved && alive.current) setError(t('desktopGuide.saveError'));
    } catch (reason) {
      console.warn('[onboarding] action failed', reason);
      if (alive.current) setError(t('desktopGuide.saveError'));
    } finally {
      pending.current = false;
      if (alive.current) setBusy(false);
    }
  };
  const advance = (answer: GuideAnswer) => {
    setEditing(false);
    persist({
      ...progress,
      name: name.trim().slice(0, 40),
      step: steps[index + 1] ?? 'finish',
      answers: { ...progress.answers, [step]: answer },
    });
  };
  const leaveGuide = (finished: boolean) =>
    run(async () => {
      // Persist before the parent swaps surfaces; the main WebView may be destroyed after the
      // handoff, so completion cannot depend on a post-handoff localStorage write.
      deferPostGuidePrompts();
      const next = { ...progress, name: name.trim().slice(0, 40), finished };
      const hasProgress = Boolean(next.name || Object.keys(next.answers).length);
      if (finished || hasProgress) {
        persist(next);
      } else if (!preview) {
        try {
          localStorage.removeItem(DESKTOP_ONBOARDING_KEY);
        } catch {
          setStorageError(true);
        }
      }
      if (isTauriHost()) await setDesktopOnboardingFinished(finished);
      await onComplete();
    });
  const finish = () => leaveGuide(true);
  const exploreFirst = () => leaveGuide(false);
  const requestPermission = () =>
    run(async () => {
      const mic = step === 'microphone';
      if (preview) {
        if (mic) setMicrophone('granted');
        else setAccessibility('granted');
        return;
      }
      if (mic && os === 'win') {
        await openSystemSettings('microphone');
      } else {
        // Accessibility authorization can remain stale after an ad-hoc update. The native
        // request command resets this app's TCC row before asking macOS to show its prompt;
        // only fall back to System Settings when the prompt did not grant access.
        const next = await (mic ? requestMicrophonePermission() : requestAccessibilityPermission());
        if (mic) setMicrophone(next);
        else setAccessibility(next);
        if (next !== 'granted') await openSystemSettings(mic ? 'microphone' : 'accessibility');
      }
      await refreshPermissions();
    });
  const continueService = () =>
    run(async () => {
      const next = await getCredentials();
      setCredentials(next);
      const configured =
        step === 'polish'
          ? next.llmConfigured
          : multimodal
            ? next.omniConfigured
            : next.asrConfigured;
      if (!configured) {
        setError(t('desktopGuide.configRequired'));
        return;
      }
      advance('done');
    });
  const back = () =>
    run(() => {
      setEditing(false);
      persist({ ...progress, step: steps[Math.max(0, index - 1)] });
    });
  const title = (id: GuideStep) =>
    t(
      `desktopGuide.${id === 'welcome' ? 'title' : id === 'recognition' && multimodal ? 'omniTitle' : `${id}Title`}`,
      {
        name: progress.name
          ? `${i18n.language.startsWith('zh') ? '，' : ', '}${progress.name}`
          : '',
      },
    );
  const body = (id: GuideStep) =>
    t(
      `desktopGuide.${id === 'welcome' ? 'intro' : id === 'recognition' && multimodal ? 'omniBody' : `${id}Body`}`,
    );
  const reply = (id: GuideStep) => {
    if (progress.answers[id] === 'later') return t('desktopGuide.laterReply');
    if (id === 'welcome') return progress.name || t('desktopGuide.startReply');
    if (id === 'mode')
      return t('desktopGuide.modeReply', { mode: t(`desktopGuide.${currentMode}`) });
    if (id === 'microphone' || id === 'accessibility') return t('desktopGuide.allowed');
    if (id === 'shortcut') return t('desktopGuide.shortcutReply');
    return t('desktopGuide.configured');
  };
  const permission = step === 'microphone' ? microphone : accessibility;
  const isPermission = step === 'microphone' || step === 'accessibility';
  const serviceConfigured =
    step === 'polish'
      ? credentials?.llmConfigured
      : multimodal
        ? credentials?.omniConfigured
        : credentials?.asrConfigured;
  const deferred = steps.filter((id) => progress.answers[id] === 'later');

  const primaryLabel =
    step === 'welcome'
      ? t('desktopGuide.start')
      : isPermission && !granted(permission)
        ? t(
            `desktopGuide.${preview ? 'previewGrant' : os === 'win' || step === 'accessibility' || permission === 'denied' || permission === 'restricted' ? 'openPermissions' : 'authorize'}`,
          )
        : step === 'recognition' || step === 'polish'
          ? t('desktopGuide.checkConfig')
          : step === 'shortcut'
            ? t('desktopGuide.keepShortcut')
            : step === 'finish'
              ? t('desktopGuide.enter')
              : t('desktopGuide.next');
  const primaryDisabled =
    busy ||
    (isPermission && (permission === null || permission === 'noDevice')) ||
    (step === 'mode' && (loading || !prefs)) ||
    (step === 'shortcut' && !prefs?.dictationHotkey.primary);
  const primaryAction = () => {
    if (step === 'finish') return finish();
    if (isPermission && !granted(permission)) return requestPermission();
    if (step === 'recognition' || step === 'polish') return continueService();
    return run(() => advance('done'));
  };

  return {
    t,
    i18n,
    preview,
    os,
    prefs,
    loading,
    preferencesError,
    refreshPreferences,
    updatePrefs,
    progress,
    name,
    setName,
    setCredentials,
    editing,
    setEditing,
    busy,
    error,
    storageError,
    form,
    stageRef,
    multimodal,
    steps,
    step,
    index,
    currentMode,
    refreshPermissions,
    run,
    advance,
    exploreFirst,
    back,
    title,
    body,
    reply,
    permission,
    isPermission,
    serviceConfigured,
    deferred,
    primaryLabel,
    primaryDisabled,
    primaryAction,
  };
}
