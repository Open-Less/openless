import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { PageSkeleton } from '../../../components/ui/PageSkeleton';
import { useHotkeySettings } from '../../../data/preferences';
import { SettingRow, Toggle } from '../../../ui/controls/SettingRow';
import { Card } from '../../../ui/layout/Card';

export function AskHistorySection() {
  const { t } = useTranslation();
  const { prefs, updatePrefs } = useHotkeySettings();
  const [saveFailed, setSaveFailed] = useState(false);
  if (!prefs) return <PageSkeleton rows={1} />;

  return (
    <Card>
      <SettingRow
        label={t('selectionAsk.history.title')}
        desc={t('selectionAsk.history.desc')}
        descriptionPlacement="inline"
      >
        <Toggle
          on={prefs.qaSaveHistory}
          onToggle={(qaSaveHistory) => {
            setSaveFailed(false);
            void updatePrefs((current) => ({ ...current, qaSaveHistory })).catch(() =>
              setSaveFailed(true),
            );
          }}
        />
      </SettingRow>
      {saveFailed && <p role="alert">{t('selectionAsk.save.historySaveFailed')}</p>}
    </Card>
  );
}
