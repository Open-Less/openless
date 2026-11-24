// Android settings: auto-check-and-download-updates toggle (the desktop equivalent lives in
// RecordingInputSection's startup group).

import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { getPlatformCapabilities, isAndroid } from '../../../runtime/platform';
import type { PlatformCapabilities } from '../../../api/contracts';
import { useHotkeySettings } from '../../../data/preferences';
import { Card } from '../../../ui/layout/Card';
import { SectionTitle } from '../../../ui/layout/SectionTitle';
import { SettingRow, Toggle } from '../../../ui/controls/SettingRow';

export function AutoUpdateSection() {
  const { t } = useTranslation();
  const { prefs, updatePrefs: savePrefs } = useHotkeySettings();
  const [platformCaps, setPlatformCaps] = useState<PlatformCapabilities | null>(null);

  useEffect(() => {
    void getPlatformCapabilities().then(setPlatformCaps);
  }, []);

  if (platformCaps?.supportsAutoUpdate !== true || !isAndroid() || !prefs) return null;

  const onAutoUpdateCheckChange = (autoUpdateCheck: boolean) =>
    savePrefs({ ...prefs, autoUpdateCheck });

  return (
    <Card>
      <SectionTitle>{t('settings.about.autoUpdateSectionTitle')}</SectionTitle>
      <p style={{ fontSize: 12.5, color: 'var(--ol-ink-3)', lineHeight: 1.55, margin: '0 0 4px' }}>
        {t('settings.about.autoUpdateCheckDescAndroid')}
      </p>
      <SettingRow label={t('settings.about.autoUpdateCheckLabelAndroid')}>
        <Toggle on={prefs.autoUpdateCheck} onToggle={onAutoUpdateCheckChange} />
      </SettingRow>
    </Card>
  );
}
