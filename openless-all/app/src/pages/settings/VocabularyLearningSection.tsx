import { useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { SelectLite } from '../../components/ui/SelectLite';
import type { UserPreferences } from '../../lib/types';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { Card } from '../_atoms';
import { btnGhostStyle, ExperimentalSectionTitle, SettingRow, Toggle } from './shared';

const defaults = { observationSeconds: 60, suggestionSeconds: 10, maxPhraseChars: 12 };
const parameters = [
  { key: 'observationSeconds', min: 10, max: 60, unit: 'seconds' },
  { key: 'suggestionSeconds', min: 5, max: 60, unit: 'seconds' },
  { key: 'maxPhraseChars', min: 2, max: 32, unit: 'characters' },
] as const;

export function VocabularyLearningSection() {
  const { t } = useTranslation();
  const { prefs, updatePrefs } = useHotkeySettings();
  const saving = useRef(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const save = async (update: (current: UserPreferences) => UserPreferences) => {
    if (saving.current) return;
    saving.current = true;
    setBusy(true);
    setError(false);
    try {
      await updatePrefs(update);
    } catch {
      setError(true);
    } finally {
      saving.current = false;
      setBusy(false);
    }
  };
  if (!prefs) return <Card>{t('common.loading')}</Card>;
  const settings = prefs.vocabularyLearningSettings ?? defaults;
  return (
    <Card>
      <ExperimentalSectionTitle badge={t('common.experimental')}>
        {t('settings.vocabularyLearning.title')}
      </ExperimentalSectionTitle>
      <p style={{ fontSize: 12, color: 'var(--ol-ink-3)', lineHeight: 1.6 }}>
        {t('settings.vocabularyLearning.description')}
      </p>
      <SettingRow label={t('settings.vocabularyLearning.enabled')}>
        <Toggle
          label={t('settings.vocabularyLearning.enabled')}
          on={prefs.vocabularyLearningEnabled}
          disabled={busy}
          onToggle={(enabled) =>
            void save((current) => ({ ...current, vocabularyLearningEnabled: enabled }))
          }
        />
      </SettingRow>
      {parameters.map(({ key, min, max, unit }) => (
        <SettingRow
          key={key}
          label={t(`settings.vocabularyLearning.${key}`)}
          desc={t(`settings.vocabularyLearning.${key}Hint`)}
        >
          <SelectLite
            ariaLabel={t(`settings.vocabularyLearning.${key}`)}
            disabled={busy}
            value={String(settings[key])}
            style={{ minWidth: 0, width: 160, maxWidth: '100%' }}
            options={Array.from({ length: max - min + 1 }, (_, index) => {
              const count = min + index;
              return {
                value: String(count),
                label: t(`settings.vocabularyLearning.${unit}`, { count }),
              };
            })}
            onChange={(value) =>
              void save((current) => ({
                ...current,
                vocabularyLearningSettings: {
                  ...(current.vocabularyLearningSettings ?? defaults),
                  [key]: Number(value),
                },
              }))
            }
          />
        </SettingRow>
      ))}
      <p style={{ fontSize: 12, color: 'var(--ol-ink-3)', lineHeight: 1.6 }}>
        {t('settings.vocabularyLearning.changeHint')}
      </p>
      <button
        type="button"
        style={btnGhostStyle}
        disabled={busy}
        onClick={() =>
          void save((current) => ({ ...current, vocabularyLearningSettings: { ...defaults } }))
        }
      >
        {t('settings.vocabularyLearning.reset')}
      </button>
      {error && <p role="alert">{t('settings.vocabularyLearning.saveError')}</p>}
    </Card>
  );
}
