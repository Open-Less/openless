// Translation.tsx — 独立的"翻译"页，从 Settings → 录音 中拆出来。
// 用户在这里：
//   - 勾选自己的工作语言（多选，用作 LLM polish/translate prompt 的前提）
//   - 选一个翻译目标语言（单选；选"不启用"则 Shift 不触发翻译）
//   - 看完整使用说明（怎么触发、按钮位置、胶囊显示）

import { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Card, PageHeader } from './_atoms';
import { SavedToast } from '../components/SavedToast';
import { SelectLite } from '../components/ui/SelectLite';
import { isTauri, listStylePacks } from '../lib/ipc';
import { filterLanguages, localizedLanguages, nativeLanguageName } from '../lib/languageCatalog';
import { Icon } from '../components/Icon';
import { getStylePackPresentation } from '../lib/stylePackPresentation';
import { isTranslationEnabled, isTranslationTargetRedundant } from '../lib/translationTarget';
import { useHotkeySettings } from '../state/HotkeySettingsContext';
import { formatComboLabel } from '../lib/hotkey';
import type { StylePack, UserPreferences } from '../lib/types';
import './Translation.css';

type SaveState = 'idle' | 'saving' | 'saved' | 'failed';

export function Translation() {
  const { t, i18n } = useTranslation();
  const { prefs, loading, error, refresh, updatePrefs: savePrefs } = useHotkeySettings();
  const [saveState, setSaveState] = useState<SaveState>('idle');
  const [saveMessage, setSaveMessage] = useState('');
  const [activeStylePack, setActiveStylePack] = useState<StylePack | null>(null);
  const activeStylePackName = activeStylePack
    ? getStylePackPresentation(activeStylePack, t).name
    : null;
  const [stylePackLoadFailed, setStylePackLoadFailed] = useState(false);
  const statusTimer = useRef<number | null>(null);
  const [languageQuery, setLanguageQuery] = useState('');
  const locale = i18n.resolvedLanguage ?? i18n.language;
  const languages = useMemo(
    () =>
      localizedLanguages(locale, [
        ...(prefs?.workingLanguages ?? []),
        prefs?.translationTargetLanguage ?? '',
      ]),
    [locale, prefs?.workingLanguages, prefs?.translationTargetLanguage],
  );
  const filteredLanguages = useMemo(
    () => filterLanguages(languages, languageQuery),
    [languages, languageQuery],
  );
  const targetOptions = useMemo(() => {
    const options = filteredLanguages.map((language) => ({
      value: language.nativeName,
      label: language.label,
    }));
    const selected = prefs?.translationTargetLanguage;
    if (selected && !options.some((option) => option.value === selected)) {
      const language = languages.find(
        (language) => language.nativeName === nativeLanguageName(selected),
      );
      options.unshift({ value: selected, label: language?.label ?? selected });
    }
    return [{ value: '', label: t('translation.target.disabled') }, ...options];
  }, [filteredLanguages, languages, prefs?.translationTargetLanguage, t]);

  useEffect(
    () => () => {
      if (statusTimer.current !== null) window.clearTimeout(statusTimer.current);
    },
    [],
  );

  useEffect(() => {
    const activeStylePackId = prefs?.activeStylePackId;
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    let request = 0;
    setActiveStylePack(null);
    setStylePackLoadFailed(false);
    const loadStyle = () => {
      const current = ++request;
      return listStylePacks()
        .then((packs) => {
          if (cancelled || current !== request) return;
          const activePack =
            packs.find((pack) => pack.active && pack.enabled) ??
            packs.find((pack) => pack.id === activeStylePackId && pack.enabled);
          setActiveStylePack(activePack ?? null);
          setStylePackLoadFailed(!activePack);
        })
        .catch((loadError) => {
          console.warn('[translation] failed to load active style pack', loadError);
          if (!cancelled && current === request) setStylePackLoadFailed(true);
        });
    };
    void loadStyle();
    if (isTauri) {
      // Restore can replace an active pack's contents without changing its ID.
      void import('@tauri-apps/api/event')
        .then(async ({ listen }) => {
          const stop = await listen('prefs:changed', () => void loadStyle());
          if (cancelled) stop();
          else unlisten = stop;
        })
        .catch((error) => console.warn('[translation] preference listener failed', error));
    }

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [prefs?.activeStylePackId]);

  const showSaveStatus = (state: SaveState, message: string, temporary = false) => {
    if (statusTimer.current !== null) {
      window.clearTimeout(statusTimer.current);
      statusTimer.current = null;
    }
    setSaveState(state);
    setSaveMessage(message);
    if (temporary) {
      statusTimer.current = window.setTimeout(() => {
        setSaveState('idle');
        setSaveMessage('');
        statusTimer.current = null;
      }, 1600);
    }
  };

  const persistPrefs = async (
    resolveNext: (current: UserPreferences) => UserPreferences,
    failureMessage: string,
  ) => {
    showSaveStatus('saving', t('common.saving'));
    try {
      await savePrefs(resolveNext);
      showSaveStatus('saved', t('common.saved'), true);
    } catch (error) {
      console.error('[translation] failed to save preferences', error);
      showSaveStatus('failed', failureMessage);
      await refresh().catch((refreshError) => {
        console.warn('[translation] failed to refresh preferences after save error', refreshError);
      });
    }
  };

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
                  borderRadius: 999,
                  border: 0,
                  background: 'var(--ol-blue)',
                  color: '#fff',
                  fontSize: 12,
                  fontWeight: 600,
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
  const translationHotkeyLabel = formatComboLabel(prefs.translationHotkey);
  // 「已启用」= 选了目标语言 **且** 该目标真的会触发翻译。目标等于唯一工作语言时后端
  // 走普通润色，状态灯不能还亮着说已启用（否则用户按 Shift 什么都没发生，无从排查）。
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
              borderRadius: 10,
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
                borderRadius: 999,
                background: 'rgba(239,68,68,0.12)',
                color: 'inherit',
                padding: '4px 10px',
                fontSize: 11,
                fontWeight: 600,
                cursor: loading ? 'not-allowed' : 'default',
                opacity: loading ? 0.64 : 1,
              }}
            >
              {loading ? t('common.loading') : t('common.retry')}
            </button>
          </div>
        )}

        <SavedToast saveState={saveState} message={saveMessage} />

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

        {/* 宽屏下「工作语言 / 目标语言」并排两栏（窄屏自动叠成一栏），
            语言 chips 用对齐的均匀网格代替自由换行，避免参差的标签云。 */}
        <div className="ol-translation-grid">
          {/* 1. 工作语言 */}
          <Card style={{ display: 'flex', flexDirection: 'column' }}>
            <div style={{ fontSize: 13.5, fontWeight: 600, marginBottom: 6 }}>
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

          {/* 2. 翻译目标语言 */}
          <Card style={{ display: 'flex', flexDirection: 'column' }}>
            <div
              style={{
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'space-between',
                marginBottom: 12,
              }}
            >
              <div style={{ fontSize: 13.5, fontWeight: 600 }}>{t('translation.target.title')}</div>
              <span
                style={{
                  padding: '2px 8px',
                  fontSize: 10.5,
                  fontWeight: 600,
                  letterSpacing: '0.04em',
                  borderRadius: 999,
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
              placeholder={t('translation.target.disabled')}
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
                <div style={{ fontSize: 12.5, fontWeight: 600, color: 'var(--ol-ink-2)' }}>
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
                  borderRadius: 999,
                  background: 'var(--ol-blue-soft)',
                  color: 'var(--ol-blue)',
                  fontSize: 11.5,
                  fontWeight: 600,
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
                  borderRadius: 10,
                  border: '0.5px solid rgba(217,119,6,0.24)',
                  background: 'rgba(217,119,6,0.08)',
                  color: 'var(--ol-warn, #b45309)',
                  fontSize: 12,
                  lineHeight: 1.55,
                }}
              >
                {t('translation.target.sameAsWorking')}
              </div>
            )}
          </Card>
        </div>

        {/* 3. 使用方法：编号步骤条，宽屏一行铺开、窄屏自动折行 */}
        <Card>
          <div style={{ fontSize: 13.5, fontWeight: 600, marginBottom: 12 }}>
            {t('translation.howto.title')}
          </div>
          <div
            style={{
              display: 'grid',
              gridTemplateColumns: 'repeat(auto-fit, minmax(150px, 1fr))',
              gap: '12px 14px',
            }}
          >
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
                    borderRadius: '50%',
                    background: 'var(--ol-blue-soft)',
                    color: 'var(--ol-blue)',
                    fontSize: 11,
                    fontWeight: 650,
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
