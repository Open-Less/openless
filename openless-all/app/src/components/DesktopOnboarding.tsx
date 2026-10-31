import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  ArrowRight,
  Check,
  ChevronLeft,
  Keyboard,
  Mic,
  ShieldCheck,
  Sparkles,
  Volume2,
} from 'lucide-react';
import {
  checkAccessibilityPermission,
  checkMicrophonePermission,
  getCredentials,
  isTauri,
  openSystemSettings,
  requestAccessibilityPermission,
  requestMicrophonePermission,
  setDictationHotkey,
} from '../lib/ipc';
import type { CredentialsStatus, PermissionStatus } from '../lib/types';
import {
  DESKTOP_ONBOARDING_KEY,
  deferPostGuidePrompts,
  setDesktopOnboardingFinished,
  guideSteps,
  newGuideProgress,
  readGuideProgress,
  saveGuideProgress,
  type GuideAnswer,
  type GuideProgress,
  type GuideStep,
} from '../lib/desktopOnboarding';
import { getHotkeyUsageHint } from '../lib/hotkey';
import { useHotkeySettings } from '../state/HotkeySettingsContext';
import { setLocalePreference, SUPPORTED_LOCALES, type SupportedLocale } from '../i18n';
import { ProviderLeaveContext, useProviderForm } from '../pages/settings/ProviderForm';
import { detectOS } from './WindowChrome';
import { ShortcutRecorder } from './ShortcutRecorder';
import { CloudSyncWelcome } from './CloudSyncSetupPrompt';
import { RevealText } from './ui/RevealText';
import { TransitionViewport } from './ui/TransitionViewport';
import { ModelSetupDialog } from './ModelSetupDialog';
import '../styles/desktop-onboarding.css';

const localeLabels = [
  '简体中文',
  '繁體中文',
  'English',
  '日本語',
  '한국어',
  'Español',
  'Français',
  'Deutsch',
];
const granted = (status: PermissionStatus | null) =>
  status === 'granted' || status === 'notApplicable';

export function DesktopOnboarding({ onComplete }: { onComplete: () => void | Promise<void> }) {
  const { t, i18n } = useTranslation();
  const preview = !isTauri;
  const os = new URLSearchParams(location.search).get('os') ?? detectOS();
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
      if (isTauri) await setDesktopOnboardingFinished(finished);
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

  return (
    <ProviderLeaveContext.Provider value={form.register}>
      <div className="ol-guide" data-os={os} data-preview={preview || undefined}>
        <header className="ol-guide-header" data-tauri-drag-region>
          <div className="ol-guide-brand">
            <img src="/AppIcon.png" width="25" height="25" alt="" />
            <span>OpenLess</span>
          </div>
          <button
            className="ol-guide-text-button"
            type="button"
            onClick={() => void exploreFirst()}
            disabled={busy}
          >
            {t('desktopGuide.skipAll')}
            <ArrowRight size={14} />
          </button>
        </header>
        <main className="ol-guide-center" aria-label={t('onboarding.welcome')}>
          <div className="ol-guide-history" aria-hidden="true">
            {index > 0 && (
              <>
                <span>{title(steps[index - 1])}</span>
                <span className="ol-guide-history-answer">{reply(steps[index - 1])}</span>
              </>
            )}
          </div>
          <div className="ol-guide-stage" ref={stageRef}>
            <TransitionViewport
              contentKey={step}
              pace="gentle"
              className="ol-guide-reading ol-thinscroll"
              style={{ overflowY: 'auto' }}
              onEntered={() =>
                stageRef.current
                  ?.querySelector<HTMLElement>(
                    '[data-transition-phase="settled"] h1, [data-transition-phase="settled"] h2',
                  )
                  ?.focus({ preventScroll: true })
              }
            >
              {[step].map((id) => {
                const active = id === step;
                return (
                  <section
                    key={id}
                    className="ol-guide-turn"
                    data-guide-step={id}
                    aria-labelledby={`guide-${id}`}
                  >
                    {id === 'welcome' ? (
                      <h1 id={`guide-${id}`} tabIndex={-1}>
                        <RevealText text={title(id)} active={active} />
                      </h1>
                    ) : (
                      <h2 id={`guide-${id}`} tabIndex={-1}>
                        <RevealText text={title(id)} active={active} />
                      </h2>
                    )}
                    <p className="ol-guide-description">
                      <RevealText text={body(id)} active={active} delay={650} />
                    </p>
                    {!active && (
                      <div className="ol-guide-reply">
                        <span>{reply(id)}</span>
                      </div>
                    )}
                    {active && (
                      <>
                        {id === 'welcome' && (
                          <>
                            <p className="ol-guide-description">
                              <RevealText text={t('desktopGuide.welcomeBody')} delay={1500} />
                            </p>
                            <form
                              className="ol-guide-welcome-form"
                              onSubmit={(event) => {
                                event.preventDefault();
                                void run(() => advance('done'));
                              }}
                            >
                              <input
                                value={name}
                                onChange={(event) => setName(event.target.value)}
                                maxLength={40}
                                placeholder={t('desktopGuide.name')}
                                aria-label={t('desktopGuide.name')}
                                autoComplete="given-name"
                              />
                            </form>
                            {!preview && (
                              <div className="ol-guide-restore">
                                <CloudSyncWelcome inline />
                              </div>
                            )}
                          </>
                        )}
                        {isPermission && (
                          <>
                            <div className="ol-guide-permission">
                              {id === 'microphone' ? <Mic size={22} /> : <ShieldCheck size={22} />}
                              <span>
                                {t(`desktopGuide.${granted(permission) ? 'allowed' : 'pending'}`)}
                              </span>
                              {granted(permission) && (
                                <Check size={18} className="ol-guide-success" />
                              )}
                            </div>
                            <p className="ol-guide-hint">
                              {permission === 'noDevice'
                                ? t('desktopGuide.unavailable')
                                : id === 'microphone' && os === 'win' && !granted(permission)
                                  ? t('desktopGuide.windowsMicrophoneHint')
                                  : t(`desktopGuide.${id}Hint`)}
                            </p>
                          </>
                        )}
                        {id === 'mode' && (
                          <>
                            <div className="ol-guide-choices" role="group" aria-label={title(id)}>
                              {(['traditional', 'multimodal'] as const).map((mode) => (
                                <button
                                  key={mode}
                                  type="button"
                                  className="ol-guide-choice"
                                  aria-pressed={currentMode === mode}
                                  disabled={busy || loading || !prefs}
                                  onClick={() =>
                                    void run(() =>
                                      updatePrefs((current) => ({
                                        ...current,
                                        pipelineMode: mode,
                                        multimodalPipelineEnabled: mode === 'multimodal',
                                      })),
                                    )
                                  }
                                >
                                  {mode === 'traditional' ? (
                                    <Volume2 size={23} />
                                  ) : (
                                    <Sparkles size={23} />
                                  )}
                                  <strong>{t(`desktopGuide.${mode}`)}</strong>
                                  <span>{t(`desktopGuide.${mode}Hint`)}</span>
                                </button>
                              ))}
                            </div>
                          </>
                        )}
                        {(id === 'recognition' || id === 'polish') && (
                          <>
                            <div className="ol-guide-service-status">
                              <span>
                                {t(
                                  `desktopGuide.${serviceConfigured ? 'configured' : 'notConfigured'}`,
                                )}
                              </span>
                              <button
                                type="button"
                                className="ol-guide-secondary"
                                onClick={() => void run(() => setEditing((value) => !value))}
                                disabled={busy}
                              >
                                {t(`desktopGuide.${editing ? 'edit' : 'configure'}`)}
                              </button>
                            </div>
                          </>
                        )}
                        {id === 'shortcut' && (
                          <>
                            <div className="ol-guide-shortcut">
                              <Keyboard size={24} />
                              {prefs && (
                                <ShortcutRecorder
                                  value={prefs.dictationHotkey}
                                  sideSpecificModifiers
                                  allowMouseButtons={os === 'win'}
                                  disabled={busy}
                                  onSave={async (binding) => {
                                    await setDictationHotkey(binding);
                                    await refreshPreferences();
                                  }}
                                />
                              )}
                            </div>
                            <p className="ol-guide-hint">
                              {t(
                                `desktopGuide.${os === 'win' ? 'windowsShortcutHint' : 'shortcutHint'}`,
                              )}
                            </p>
                            {prefs && (
                              <p className="ol-guide-description">
                                {getHotkeyUsageHint(prefs.hotkey, undefined, prefs.dictationHotkey)}
                              </p>
                            )}
                          </>
                        )}
                        {id === 'finish' && (
                          <>
                            <div className="ol-guide-summary">
                              {steps
                                .filter((item) => !['welcome', 'mode', 'finish'].includes(item))
                                .map((item) => (
                                  <div key={item}>
                                    <span>{title(item)}</span>
                                    <span
                                      className={
                                        progress.answers[item] === 'done' ? 'ol-guide-success' : ''
                                      }
                                    >
                                      {t(
                                        `desktopGuide.${progress.answers[item] === 'done' ? 'allSet' : 'deferred'}`,
                                      )}
                                    </span>
                                  </div>
                                ))}
                            </div>
                            {deferred.length > 0 && (
                              <p className="ol-guide-hint">{t('desktopGuide.finishPending')}</p>
                            )}
                          </>
                        )}
                        {(error || preferencesError) && (
                          <div className="ol-guide-error" role="alert">
                            {error || t('desktopGuide.saveError')}
                            <button
                              type="button"
                              className="ol-guide-text-button"
                              onClick={() => void run(refreshPreferences)}
                              disabled={busy}
                            >
                              {t('common.retry')}
                            </button>
                          </div>
                        )}
                        {storageError && (
                          <p className="ol-guide-hint" role="status">
                            {t('desktopGuide.storageError')}
                          </p>
                        )}
                      </>
                    )}
                  </section>
                );
              })}
            </TransitionViewport>
          </div>
          <div className="ol-guide-interaction">
            <div className="ol-guide-actions">
              {step !== 'welcome' && step !== 'finish' && (
                <button
                  type="button"
                  className="ol-guide-text-button"
                  disabled={busy}
                  onClick={() => void run(() => advance('later'))}
                >
                  {t('desktopGuide.later')}
                </button>
              )}
              {isPermission && !preview && !granted(permission) && (
                <button
                  type="button"
                  className="ol-guide-text-button"
                  disabled={busy}
                  onClick={() => void run(refreshPermissions)}
                >
                  {t('desktopGuide.recheck')}
                </button>
              )}
              <button
                type="button"
                className="ol-guide-primary"
                disabled={primaryDisabled}
                onClick={() => void primaryAction()}
              >
                {primaryLabel}
                <ArrowRight size={16} />
              </button>
            </div>
            <div className="ol-guide-step-footer">
              {index > 0 && (
                <button
                  type="button"
                  className="ol-guide-text-button"
                  disabled={busy}
                  onClick={() => void back()}
                >
                  <ChevronLeft size={14} />
                  {t('desktopGuide.back')}
                </button>
              )}
              <span>{t('desktopGuide.step', { current: index + 1, total: steps.length })}</span>
            </div>
          </div>
        </main>
        <ModelSetupDialog
          open={editing}
          kind={step === 'polish' ? 'llm' : multimodal ? 'omni' : 'asr'}
          onClose={async () => {
            setCredentials(await getCredentials());
            setEditing(false);
          }}
        />
        <footer className="ol-guide-footer">
          <span>{preview ? t('desktopGuide.preview') : ''}</span>
          <label>
            <span className="ol-sr-only">{t('desktopGuide.language')}</span>
            <select
              aria-label={t('desktopGuide.language')}
              value={i18n.resolvedLanguage}
              onChange={(event) =>
                void run(() =>
                  setLocalePreference(event.target.value as SupportedLocale).then(() => undefined),
                )
              }
            >
              {SUPPORTED_LOCALES.map((locale, i) => (
                <option key={locale} value={locale}>
                  {localeLabels[i]}
                </option>
              ))}
            </select>
          </label>
        </footer>
      </div>
    </ProviderLeaveContext.Provider>
  );
}
