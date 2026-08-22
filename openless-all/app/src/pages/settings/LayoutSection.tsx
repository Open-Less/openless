import { useTranslation } from 'react-i18next';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { Card } from '../_atoms';
import { SettingRow, Toggle } from './shared';

export function LayoutSection() {
  const { t } = useTranslation();
  const { prefs, updatePrefs } = useHotkeySettings();

  if (!prefs) return null;

  return (
    <Card>
      <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 6 }}>
        {t('settings.layout.title')}
      </div>
      <SettingRow
        label={t('settings.theme.stackedRowLayoutLabel')}
        desc={t('settings.theme.stackedRowLayoutDesc')}
      >
        <Toggle
          on={prefs.stackedRowLayout === true}
          onToggle={next =>
            void updatePrefs(current => ({ ...current, stackedRowLayout: next }))
          }
        />
      </SettingRow>
      <SettingRow
        label={t('settings.theme.conservativeLayoutLabel')}
        desc={t('settings.theme.conservativeLayoutDesc')}
      >
        <Toggle
          on={prefs.conservativeLayout === true}
          onToggle={next =>
            void updatePrefs(current => ({ ...current, conservativeLayout: next }))
          }
        />
      </SettingRow>
    </Card>
  );
}
