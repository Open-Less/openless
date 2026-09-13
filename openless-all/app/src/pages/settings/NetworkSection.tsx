// 服务 → 网络：全局系统代理开关（issue #869）。
// 关闭后所有 reqwest 请求直连，适合国内模型直连延迟更低的场景；
// 实时语音流（WebSocket）与 Less Computer 子进程不受此开关影响。
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
