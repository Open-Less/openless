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
import { getHotkeyUsageHint, setDictationHotkey } from '../../api/hotkeys';
import { getCredentials } from '../../api/providers';
import { CloudSyncWelcome } from '../../components/CloudSyncSetupPrompt';
import { ModelSetupDialog } from './ModelSetupDialog';
import { ShortcutRecorder } from '../../components/ShortcutRecorder';
import { RevealText } from '../../components/ui/RevealText';
import { TransitionViewport } from '../../components/ui/TransitionViewport';
import { setLocalePreference, SUPPORTED_LOCALES, type SupportedLocale } from '../../i18n';
import { ProviderLeaveContext } from '../providers/ProviderForm';
import './desktop-onboarding.css';
import { granted, useDesktopGuide } from './useDesktopGuide';

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
export function DesktopOnboarding({ onComplete }: { onComplete: () => void | Promise<void> }) {
  const {
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
  } = useDesktopGuide({ onComplete });
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
