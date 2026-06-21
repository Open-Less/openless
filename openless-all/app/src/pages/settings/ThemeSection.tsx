import { useMemo, useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { SelectLite } from '../../components/ui/SelectLite';
import { Card } from '../_atoms';
import { SettingRow, Toggle } from './shared';
import {
  readThemePreference,
  setThemePreference,
  type ThemePreference,
} from '../../lib/themeMode';

export function ThemeSection() {
  const { t } = useTranslation();
  const { prefs, updatePrefs } = useHotkeySettings();
  const [pref, setPref] = useState<ThemePreference>(
    () => prefs?.themeMode ?? readThemePreference(),
  );

  useEffect(() => {
    if (prefs?.themeMode) setPref(prefs.themeMode);
  }, [prefs?.themeMode]);

  const options = useMemo(
    () => [
      { value: 'system' as const, label: t('settings.theme.system') },
      { value: 'light' as const, label: t('settings.theme.light') },
      { value: 'dark' as const, label: t('settings.theme.dark') },
    ],
    [t],
  );

  const apply = async (next: ThemePreference) => {
    setPref(next);
    setThemePreference(next);
    await updatePrefs(current => {
      if (current.themeMode === next) return current;
      return { ...current, themeMode: next };
    });
  };

  return (
    <Card>
      <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 6 }}>
        {t('settings.theme.title')}
      </div>
      <SettingRow label={t('settings.theme.label')}>
        <SelectLite
          value={pref}
          onChange={next => void apply(next as ThemePreference)}
          options={options}
          ariaLabel={t('settings.theme.label')}
          style={{ maxWidth: 220, minWidth: 200 }}
        />
      </SettingRow>
      {prefs && (
        <SettingRow label={t('settings.theme.overviewActivityLabel')}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 10, minWidth: 0 }}>
            <Toggle
              on={prefs.showOverviewActivityHeatmap !== false}
              onToggle={next => void updatePrefs({ ...prefs, showOverviewActivityHeatmap: next })}
            />
            <span style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.45 }}>
              {t('settings.theme.overviewActivityDesc')}
            </span>
          </div>
        </SettingRow>
      )}
    </Card>
  );
}
