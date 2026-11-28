import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useHotkeySettings } from '../../../data/preferences';
import { SettingRow, Toggle } from '../../../ui/controls/SettingRow';
import { Card } from '../../../ui/layout/Card';
import { ExperimentalSectionTitle } from '../../../ui/layout/ExperimentalSectionTitle';
import './dictation-references.css';

export function DictationReferencesSection() {
  const { t } = useTranslation();
  const { prefs, updatePrefs } = useHotkeySettings();
  const [saving, setSaving] = useState(false);
  if (!prefs) return null;

  const save = async (dictationReferencesEnabled: boolean) => {
    setSaving(true);
    try {
      await updatePrefs((current) => ({ ...current, dictationReferencesEnabled }));
    } catch {
      // The preferences provider rolls back and reports failed writes.
    } finally {
      setSaving(false);
    }
  };

  return (
    <Card className="ol-dictation-references">
      <ExperimentalSectionTitle badge={t('common.experimental')}>
        {t('settings.advancedPages.dictationReferencesTitle')}
      </ExperimentalSectionTitle>
      <SettingRow
        label={t('settings.dictationReferences.enabledLabel')}
        desc={t('settings.dictationReferences.enabledDesc')}
        descriptionPlacement="inline"
      >
        <Toggle
          on={prefs.dictationReferencesEnabled ?? false}
          disabled={saving}
          onToggle={(next) => void save(next)}
        />
      </SettingRow>
    </Card>
  );
}
