// 高级 → Beta 渠道。Toggle 控制后台 AutoUpdateGate 跟随 stable/beta；
// 「检查 Beta 更新」按钮始终可用，与 Toggle 状态无关。
// 关于页的「检查正式版更新」固定查 stable，两者互不影响。

import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { getPlatformCapabilities, getUpdateChannel, setUpdateChannel, type UpdateChannel } from '../../lib/ipc';
import type { PlatformCapabilities } from '../../lib/types';
import { Card } from '../_atoms';
import { SectionTitle, SettingRow, Toggle } from './shared';
import { CheckUpdateButton } from './CheckUpdateButton';

export function BetaChannelSection() {
  const { t } = useTranslation();
  const [channel, setChannel] = useState<UpdateChannel>('stable');
  const [platformCaps, setPlatformCaps] = useState<PlatformCapabilities | null>(null);

  useEffect(() => {
    void getPlatformCapabilities().then(setPlatformCaps);
  }, []);

  useEffect(() => {
    let cancelled = false;
    void getUpdateChannel()
      .then(c => { if (!cancelled) setChannel(c); })
      .catch(() => { /* fall back to stable already in initial state */ });
    return () => { cancelled = true; };
  }, []);

  const onToggle = async (next: boolean) => {
    const target: UpdateChannel = next ? 'beta' : 'stable';
    setChannel(target);
    try {
      await setUpdateChannel(target);
    } catch {
      setChannel(target === 'beta' ? 'stable' : 'beta');
    }
  };

  if (platformCaps?.supportsAutoUpdate !== true) return null;

  return (
    <Card>
      <SectionTitle>{t('settings.about.betaChannelLabel')}</SectionTitle>
      <p style={{ fontSize: 12.5, color: 'var(--ol-ink-3)', lineHeight: 1.55, margin: '0 0 4px' }}>
        {t('settings.about.betaChannelDesc')}
      </p>
      <SettingRow label={t('settings.about.betaChannelToggleLabel')}>
        <Toggle on={channel === 'beta'} onToggle={onToggle} />
      </SettingRow>
      <div style={{ display: 'flex', justifyContent: 'flex-end', paddingTop: 4 }}>
        <CheckUpdateButton channel="beta" />
      </div>
    </Card>
  );
}
