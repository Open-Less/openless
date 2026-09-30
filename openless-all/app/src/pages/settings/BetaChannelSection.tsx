// Advanced → Beta channel. The Toggle controls whether the background AutoUpdateGate
// follows stable/beta; the "check for Beta updates" button always works regardless of the
// Toggle state. The About page's "check for stable updates" always checks stable — the two
// don't interfere with each other.

import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  getPlatformCapabilities,
  getUpdateChannel,
  setUpdateChannel,
  type UpdateChannel,
} from '../../lib/ipc';
import type { PlatformCapabilities } from '../../lib/types';
import { Card } from '../_atoms';
import { SectionTitle, SettingRow, Toggle } from './shared';
import { CheckUpdateButton } from './CheckUpdateButton';

export function BetaChannelSection() {
  const { t } = useTranslation();
  const [channel, setChannel] = useState<UpdateChannel>('stable');
  const [autoCheckChannel, setAutoCheckChannel] = useState<UpdateChannel | null>(null);
  const [platformCaps, setPlatformCaps] = useState<PlatformCapabilities | null>(null);

  useEffect(() => {
    void getPlatformCapabilities().then(setPlatformCaps);
  }, []);

  useEffect(() => {
    let cancelled = false;
    void getUpdateChannel()
      .then((c) => {
        if (!cancelled) setChannel(c);
      })
      .catch(() => {
        /* fall back to stable already in initial state */
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const onToggle = async (next: boolean) => {
    const target: UpdateChannel = next ? 'beta' : 'stable';
    setChannel(target);
    try {
      await setUpdateChannel(target);
    } catch {
      setChannel(target === 'beta' ? 'stable' : 'beta');
      return;
    }
    setAutoCheckChannel(target);
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
        <CheckUpdateButton channel="beta" autoCheckChannel={autoCheckChannel} />
      </div>
    </Card>
  );
}
