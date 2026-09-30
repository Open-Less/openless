// Services → Network: global system proxy toggle (issue #869).
// When off, all reqwest requests connect directly — suited to lower-latency direct access for domestic models;
// realtime voice streams (WebSocket) and Less Computer subprocesses are unaffected by this toggle.
import { useTranslation } from 'react-i18next';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { Card } from '../_atoms';
import { SectionTitle, SettingRow, Toggle } from './shared';

export function NetworkSection() {
  const { t } = useTranslation();
  const { prefs, updatePrefs } = useHotkeySettings();
  if (!prefs) return null;

  return (
    <Card>
      <SectionTitle>{t('settings.network.title')}</SectionTitle>
      <SettingRow
        label={t('settings.network.useSystemProxyLabel')}
        desc={t('settings.network.useSystemProxyDesc')}
      >
        <Toggle
          on={prefs.useSystemProxy}
          onToggle={(next) => void updatePrefs((current) => ({ ...current, useSystemProxy: next }))}
        />
      </SettingRow>
    </Card>
  );
}
