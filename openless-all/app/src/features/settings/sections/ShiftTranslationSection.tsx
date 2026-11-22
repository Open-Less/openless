import { useTranslation } from 'react-i18next';
import { useHotkeySettings } from '../../../data/preferences';
import { Card } from '../../../ui/layout/Card';
import { SectionTitle } from '../../../ui/layout/SectionTitle';
import { SettingRow } from '../../../ui/controls/SettingRow';
import { Select } from '../../../ui/controls/Select';

export function ShiftTranslationSection() {
  const { t } = useTranslation();
  const { prefs, updatePrefs } = useHotkeySettings();
  if (!prefs) return null;
  const count = prefs.translationShiftTapCount ?? 1;
  const counts = [...new Set([1, 2, 3, count])].sort((a, b) => a - b);
  return (
    <Card>
      <SectionTitle>{t('shiftTranslation.title')}</SectionTitle>
      <SettingRow label={t('shiftTranslation.count')}>
        <Select
          ariaLabel={t('shiftTranslation.count')}
          value={String(count)}
          options={counts.map((value) => ({
            value: String(value),
            label: t('shiftTranslation.times', { count: value }),
          }))}
          onChange={(value) => {
            void updatePrefs((current) => ({
              ...current,
              translationShiftTapCount: Number(value),
            }));
          }}
        />
      </SettingRow>
      <p className="ol-setting-row-description">{t('shiftTranslation.scope')}</p>
    </Card>
  );
}
