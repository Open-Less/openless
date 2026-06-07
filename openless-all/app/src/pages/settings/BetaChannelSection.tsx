// 高级 → 加入 Beta 渠道。单独成一节，固定放在「高级」页最下面。
//
// 打开后写 prefs.update_channel='beta'：后台 AutoUpdateGate 自动更新随之走 Beta，
// 同时本节出现「检查更新」按钮 —— 手动查测试版更新（CheckUpdateButton channel='beta'）。
// 关于页的检查更新按钮固定查正式版（channel='stable'），两者互不影响。

import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { getPlatformCapabilities, getUpdateChannel, setUpdateChannel, type UpdateChannel } from '../../lib/ipc';
import type { PlatformCapabilities } from '../../lib/types';
import { Card } from '../_atoms';
import { SectionTitle, Toggle } from './shared';
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
      // 写入失败时回滚 UI，免得用户以为切成功了。
      setChannel(target === 'beta' ? 'stable' : 'beta');
    }
  };

  if (platformCaps?.supportsAutoUpdate !== true) return null;

  return (
    <Card>
      <SectionTitle>{t('settings.about.betaChannelLabel')}</SectionTitle>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12, paddingTop: 2 }}>
        <span style={{ fontSize: 12.5, color: 'var(--ol-ink-3)' }}>
          {t('settings.about.betaChannelDesc')}
        </span>
        <div style={{ display: 'flex', alignItems: 'center', gap: 12, flexShrink: 0 }}>
          {channel === 'beta' && <CheckUpdateButton channel="beta" />}
          <Toggle on={channel === 'beta'} onToggle={onToggle} />
        </div>
      </div>
    </Card>
  );
}
