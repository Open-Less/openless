import { formatComboLabel } from '../../api/hotkeys';
import { Icon } from '../../components/Icon';
import { nativeLanguageName } from '../../lib/languageCatalog';
import { isTranslationEnabled, isTranslationTargetRedundant } from '../../lib/translationTarget';
import { Card } from '../../ui/layout/Card';
import { PageHeader } from '../../ui/layout/PageHeader';
import { SelectLite } from '../../ui/controls/SelectLite';
import './Translation.css';
import { useTranslationSettings } from './useTranslationSettings';

export function Translation() {
  const {
    t,
    prefs,
    loading,
    error,
    refresh,
    activeStylePackName,
    stylePackLoadFailed,
    languageQuery,
    setLanguageQuery,
    translationShortcutLabel,
    filteredLanguages,
    targetOptions,
    persistPrefs,
  } = useTranslationSettings();
  if (!prefs) {
    return (
      <>
        <PageHeader title={t('translation.title')} desc={t('translation.desc')} />
        <Card>
          {error ? (
            <div role="alert" style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
              <div style={{ fontSize: 12, color: 'var(--ol-red, #ef4444)', lineHeight: 1.5 }}>
                {t('common.settingsLoadFailed')}：{error}
              </div>
              <button
                type="button"
                onClick={() => {
                  void refresh();
                }}
                disabled={loading}
                style={{
                  alignSelf: 'flex-start',
                  padding: '6px 12px',
                  borderRadius: 'var(--ol-radius)',
                  border: 0,
                  background: 'var(--ol-blue)',
                  color: '#fff',
                  fontSize: 12,
                  fontWeight: 'var(--ol-font-weight-semibold)',
                  cursor: loading ? 'not-allowed' : 'default',
                  opacity: loading ? 0.64 : 1,
                }}
              >
                {loading ? t('common.loading') : t('common.retry')}
              </button>
            </div>
          ) : (
            <div style={{ fontSize: 12, color: 'var(--ol-ink-4)' }}>{t('common.loading')}</div>
          )}
        </Card>
      </>
    );
  }

  const toggleWorkingLanguage = (lang: string) => {
    void persistPrefs((current) => {
      const selected = current.workingLanguages.some((value) => nativeLanguageName(value) === lang);
      return {
        ...current,
        workingLanguages: selected
          ? current.workingLanguages.filter((value) => nativeLanguageName(value) !== lang)
          : [...current.workingLanguages, lang],
      };
    }, t('translation.save.workingFailed'));
  };
  const onTargetChange = (translationTargetLanguage: string) => {
    void persistPrefs(
      (current) => ({ ...current, translationTargetLanguage }),
      t('translation.save.targetFailed'),
    );
  };

  const triggerLabel = formatComboLabel(prefs.dictationHotkey);
  const translationHotkeyLabel = translationShortcutLabel;
  // "Enabled" = a target language is chosen **and** that target really triggers translation.
  // When the target equals the only working language the backend does plain polishing, so the
  // status light must not claim enabled (otherwise pressing Shift does nothing and gives the
  // user no way to debug it).
  const redundantTarget = isTranslationTargetRedundant(
    prefs.translationTargetLanguage,
    prefs.workingLanguages,
  );
  const enabled = isTranslationEnabled(prefs.translationTargetLanguage) && !redundantTarget;

  return (
    <>
      <PageHeader title={t('translation.title')} desc={t('translation.desc')} />

      <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
        {error && (
          <div
            role="alert"
            style={{
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              gap: 10,
              padding: '8px 12px',
              borderRadius: 'var(--ol-r-md)',
              border: '0.5px solid rgba(239,68,68,0.22)',
              background: 'rgba(239,68,68,0.07)',
              color: 'var(--ol-red, #ef4444)',
              fontSize: 11.5,
              lineHeight: 1.5,
            }}
          >
            <span>
              {t('common.settingsLoadFailed')}：{error}
            </span>
            <button
              type="button"
              onClick={() => {
                void refresh();
              }}
              disabled={loading}
              style={{
                flex: '0 0 auto',
                border: 0,
                borderRadius: 'var(--ol-radius)',
                background: 'rgba(239,68,68,0.12)',
                color: 'inherit',
                padding: '4px 10px',
                fontSize: 11,
                fontWeight: 'var(--ol-font-weight-semibold)',
                cursor: loading ? 'not-allowed' : 'default',
                opacity: loading ? 0.64 : 1,
              }}
            >
              {loading ? t('common.loading') : t('common.retry')}
            </button>
          </div>
        )}

        <div className="ol-translation-toolbar">
          <label className="ol-language-search">
            <Icon name="search" size={15} />
            <input
              type="search"
              value={languageQuery}
              onChange={(event) => setLanguageQuery(event.target.value)}
              placeholder={t('translation.searchLanguages')}
              aria-label={t('translation.searchLanguages')}
            />
          </label>
          <span className="ol-translation-count">
            {t('translation.selectedLanguages', { count: prefs.workingLanguages.length })}
          </span>
        </div>

        {/* On wide screens "working language / target language" sit side by side (narrow screens stack);
            language chips use an aligned uniform grid instead of free wrapping, avoiding a ragged tag cloud. */}
        <div className="ol-translation-grid">
          {/* 1. Working languages */}
          <Card style={{ display: 'flex', flexDirection: 'column' }}>
            <div
              style={{
                fontSize: 13.5,
                fontWeight: 'var(--ol-font-weight-semibold)',
                marginBottom: 6,
              }}
            >
              {t('translation.working.title')}
            </div>
            <p className="ol-translation-description">{t('translation.working.desc')}</p>
            <div
              className="ol-language-grid ol-thinscroll"
              role="group"
              aria-label={t('translation.working.title')}
            >
              {filteredLanguages.map((language) => {
                const checked = prefs.workingLanguages.some(
                  (value) => nativeLanguageName(value) === language.nativeName,
                );
                return (
                  <button
                    key={language.nativeName}
                    type="button"
                    className="ol-language-choice"
                    data-selected={checked}
                    aria-pressed={checked}
                    title={language.label}
                    onClick={() => toggleWorkingLanguage(language.nativeName)}
                  >
                    <span>
                      <span className="ol-language-name" dir="auto">
                        {language.displayName}
                      </span>
                      {language.displayName !== language.nativeName && (
                        <span className="ol-language-native" dir="auto">
                          {language.nativeName}
                        </span>
                      )}
                    </span>
                    <span className="ol-language-check">
                      {checked && <Icon name="check" size={13} />}
                    </span>
                  </button>
                );
              })}
              {filteredLanguages.length === 0 && (
                <p className="ol-language-empty" role="status">
                  {t('translation.noMatchingLanguages')}
                </p>
              )}
            </div>
            <p className="ol-translation-language-hint">{t('translation.languageSupportHint')}</p>
          </Card>

          {/* 2. Translation target language */}
          <Card style={{ display: 'flex', flexDirection: 'column' }}>
            <div
              style={{
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'space-between',
                marginBottom: 12,
              }}
            >
              <div style={{ fontSize: 13.5, fontWeight: 'var(--ol-font-weight-semibold)' }}>
                {t('translation.target.title')}
              </div>
              <span
                style={{
                  padding: '2px 8px',
                  fontSize: 10.5,
                  fontWeight: 'var(--ol-font-weight-semibold)',
                  letterSpacing: '0.04em',
                  borderRadius: 'var(--ol-radius)',
                  background: enabled ? 'var(--ol-blue-soft)' : 'var(--ol-control-muted)',
                  color: enabled ? 'var(--ol-blue)' : 'var(--ol-ink-4)',
                  textTransform: 'uppercase',
                }}
              >
                {enabled ? t('translation.statusEnabled') : t('translation.statusDisabled')}
              </span>
            </div>
            <SelectLite
              value={prefs.translationTargetLanguage}
              onChange={onTargetChange}
              options={targetOptions}
              placeholder={t('translation.target.disabled', {
                shortcut: translationShortcutLabel,
              })}
              ariaLabel={t('translation.target.title')}
              style={{ width: '100%', fontSize: 13 }}
            />
            <div
              style={{
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'space-between',
                gap: 12,
                marginTop: 12,
                paddingTop: 12,
                borderTop: '0.5px solid var(--ol-line)',
              }}
            >
              <div style={{ minWidth: 0 }}>
                <div
                  style={{
                    fontSize: 12.5,
                    fontWeight: 'var(--ol-font-weight-semibold)',
                    color: 'var(--ol-ink-2)',
                  }}
                >
                  {t('translation.style.title')}
                </div>
                <div
                  style={{ marginTop: 2, fontSize: 12, color: 'var(--ol-ink-4)', lineHeight: 1.5 }}
                >
                  {t('translation.style.desc')}
                </div>
              </div>
              <span
                role="status"
                title={stylePackLoadFailed ? undefined : (activeStylePackName ?? undefined)}
                style={{
                  flex: '0 0 auto',
                  maxWidth: 180,
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                  whiteSpace: 'nowrap',
                  padding: '3px 9px',
                  borderRadius: 'var(--ol-radius)',
                  background: 'var(--ol-blue-soft)',
                  color: 'var(--ol-blue)',
                  fontSize: 11.5,
                  fontWeight: 'var(--ol-font-weight-semibold)',
                }}
              >
                {stylePackLoadFailed
                  ? t('translation.style.unavailable')
                  : (activeStylePackName ?? t('common.loading'))}
              </span>
            </div>
            {redundantTarget && (
              <div
                role="status"
                style={{
                  marginTop: 10,
                  padding: '8px 12px',
                  borderRadius: 'var(--ol-r-md)',
                  border: '0.5px solid rgba(217,119,6,0.24)',
                  background: 'rgba(217,119,6,0.08)',
                  color: 'var(--ol-warn, #b45309)',
                  fontSize: 12,
                  lineHeight: 1.55,
                }}
              >
                {t('translation.target.sameAsWorking', { shortcut: translationShortcutLabel })}
              </div>
            )}
          </Card>
        </div>

        {/* 3. How to use: numbered step strip, one row on wide screens, wraps on narrow ones */}
        <Card>
          <div
            style={{
              fontSize: 13.5,
              fontWeight: 'var(--ol-font-weight-semibold)',
              marginBottom: 12,
            }}
          >
            {t('translation.howto.title')}
          </div>
          <div className="ol-grid-12 ol-grid-steps">
            {[
              t('translation.howto.step1', { trigger: triggerLabel }),
              t('translation.howto.step2', { trigger: triggerLabel }),
              t('translation.howto.step3', { shortcut: translationHotkeyLabel }),
              t('translation.howto.step4', { trigger: triggerLabel }),
              t('translation.howto.step5'),
            ].map((step, index) => (
              <div key={index} style={{ display: 'flex', alignItems: 'flex-start', gap: 8 }}>
                <span
                  style={{
                    flex: '0 0 auto',
                    width: 18,
                    height: 18,
                    marginTop: 1,
                    display: 'inline-flex',
                    alignItems: 'center',
                    justifyContent: 'center',
                    borderRadius: 'var(--ol-radius)',
                    background: 'var(--ol-blue-soft)',
                    color: 'var(--ol-blue)',
                    fontSize: 11,
                    fontWeight: 'var(--ol-font-weight-demibold)',
                    fontVariantNumeric: 'tabular-nums',
                  }}
                >
                  {index + 1}
                </span>
                <span style={{ fontSize: 12.5, color: 'var(--ol-ink-2)', lineHeight: 1.55 }}>
                  {step}
                </span>
              </div>
            ))}
          </div>
        </Card>
      </div>
    </>
  );
}
