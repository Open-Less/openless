import { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { StylePack, UserPreferences } from '../../api/contracts';
import { formatComboLabel } from '../../api/hotkeys';
import { listenPreferenceChanges } from '../../api/preferences';
import { listStylePacks } from '../../api/style-packs';
import { useHotkeySettings } from '../../data/preferences';
import { filterLanguages, localizedLanguages, nativeLanguageName } from '../../lib/languageCatalog';
import { getStylePackPresentation } from '../../lib/stylePackPresentation';
import { useInteraction } from '../../ui/overlays/InteractionProvider';

export function useTranslationSettings() {
  const { t, i18n } = useTranslation();
  const { prefs, loading, error, refresh, updatePrefs: savePrefs } = useHotkeySettings();
  const { notify } = useInteraction('translation');
  const saveSequence = useRef(0);
  const [activeStylePack, setActiveStylePack] = useState<StylePack | null>(null);
  const activeStylePackName = activeStylePack
    ? getStylePackPresentation(activeStylePack, t).name
    : null;
  const [stylePackLoadFailed, setStylePackLoadFailed] = useState(false);
  const [languageQuery, setLanguageQuery] = useState('');
  const locale = i18n.resolvedLanguage ?? i18n.language;
  // The translation trigger key is user-configurable; UI text must not hardcode
  // "Shift" (issue #1358).
  const translationShortcutLabel = formatComboLabel(
    prefs?.translationHotkey ?? { primary: 'Shift', modifiers: [] },
  );
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
    return [
      {
        value: '',
        label: t('translation.target.disabled', { shortcut: translationShortcutLabel }),
      },
      ...options,
    ];
  }, [filteredLanguages, languages, prefs?.translationTargetLanguage, t, translationShortcutLabel]);

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
    // Restore can replace an active pack without changing its ID.
    void listenPreferenceChanges(() => {
      if (!cancelled) void loadStyle();
    })
      .then((stop) => {
        if (cancelled) stop();
        else unlisten = stop;
      })
      .catch((error) => console.warn('[translation] preference listener failed', error));

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [prefs?.activeStylePackId]);

  const persistPrefs = async (
    resolveNext: (current: UserPreferences) => UserPreferences,
    failureMessage: string,
  ) => {
    const sequence = ++saveSequence.current;
    const notice = notify({
      key: 'translation-write',
      state: 'saving',
      message: t('common.saving'),
    });
    try {
      await savePrefs(resolveNext);
      if (sequence === saveSequence.current)
        notice.update({ state: 'saved', message: t('common.saved') });
    } catch (error) {
      console.error('[translation] failed to save preferences', error);
      if (sequence === saveSequence.current)
        notice.update({ state: 'failed', message: failureMessage });
      await refresh().catch((refreshError) => {
        console.warn('[translation] failed to refresh preferences after save error', refreshError);
      });
    }
  };

  return {
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
  };
}
