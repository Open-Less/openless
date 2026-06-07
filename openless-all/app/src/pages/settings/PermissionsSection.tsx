// 权限/连通性面板：麦克风 / 辅助功能 / 全局热键 / Windows IME / 网络。
// 内含三个状态 Pill + 适配器名称翻译辅助函数。

import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../../components/Icon';
import {
  checkAccessibilityPermission,
  checkMicrophonePermission,
  checkNetwork,
  getHotkeyStatus,
  getWindowsImeStatus,
  openSystemSettings,
  requestAccessibilityPermission,
  requestMicrophonePermission,
} from '../../lib/ipc';
import type { NetworkCheckResult } from '../../lib/ipc';
import { getPlatformCapabilities } from '../../lib/platform';
import type {
  HotkeyStatus,
  PermissionStatus,
  PlatformCapabilities,
  WindowsImeStatus,
} from '../../lib/types';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { Btn, Card, Pill } from '../_atoms';
import { SettingRow } from './shared';

export function PermissionsSection() {
  const { t } = useTranslation();
  const [accessibility, setAccessibility] = useState<PermissionStatus | 'loading'>('loading');
  const [microphone, setMicrophone] = useState<PermissionStatus | 'loading'>('loading');
  const [hotkey, setHotkey] = useState<HotkeyStatus | null>(null);
  const [windowsIme, setWindowsIme] = useState<WindowsImeStatus | null>(null);
  const [network, setNetwork] = useState<NetworkCheckResult | null>(null);
  const [platformCaps, setPlatformCaps] = useState<PlatformCapabilities | null>(null);
  const { capability } = useHotkeySettings();

  useEffect(() => {
    void getPlatformCapabilities().then(setPlatformCaps);
  }, []);

  const refreshPermissions = async () => {
    const [a, m] = await Promise.all([
      checkAccessibilityPermission(),
      checkMicrophonePermission(),
    ]);
    setAccessibility(a);
    setMicrophone(m);
  };

  const refreshHotkey = async () => {
    setHotkey(await getHotkeyStatus());
  };

  const refreshWindowsIme = async () => {
    setWindowsIme(await getWindowsImeStatus());
  };

  const refreshNetwork = async () => {
    try {
      setNetwork(await checkNetwork());
    } catch {
      setNetwork({ online: false, latencyMs: null });
    }
  };

  useEffect(() => {
    refreshPermissions();
    if (platformCaps?.supportsDesktopHotkey === true) {
      refreshHotkey();
    }
    if (platformCaps?.platform !== 'android') {
      refreshWindowsIme();
    }
    refreshNetwork();
    const hotkeyId = platformCaps?.supportsDesktopHotkey === true
      ? window.setInterval(refreshHotkey, 1000)
      : undefined;
    // 麦克风检查会短暂打开输入流，避免每秒探测导致隐私指示器频繁闪烁。
    const permissionId = window.setInterval(refreshPermissions, 10000);
    const networkId = window.setInterval(refreshNetwork, 30000);
    const onFocus = () => {
      refreshPermissions();
      if (platformCaps?.supportsDesktopHotkey === true) {
        refreshHotkey();
      }
      if (platformCaps?.platform !== 'android') {
        refreshWindowsIme();
      }
      refreshNetwork();
    };
    window.addEventListener('focus', onFocus);
    return () => {
      if (hotkeyId !== undefined) window.clearInterval(hotkeyId);
      window.clearInterval(permissionId);
      window.clearInterval(networkId);
      window.removeEventListener('focus', onFocus);
    };
  }, [platformCaps?.platform, platformCaps?.supportsDesktopHotkey]);

  const reRequestAccessibility = async () => {
    await requestAccessibilityPermission();
    refreshPermissions();
  };

  const reRequestMicrophone = async () => {
    if (microphone === 'denied' || microphone === 'restricted') {
      await openSystemSettings('microphone');
      refreshPermissions();
      return;
    }
    const status = await requestMicrophonePermission();
    setMicrophone(status);
    if (status === 'denied' || status === 'restricted') {
      await openSystemSettings('microphone');
    }
    refreshPermissions();
  };

  return (
    <Card>
      <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 6 }}>{t('settings.permissions.title')}</div>
      <SettingRow label={t('settings.permissions.micLabel')}>
        <div style={{ display: 'flex', gap: 8, alignItems: 'center', justifyContent: 'flex-end', width: '100%' }}>
          <PermissionPill status={microphone} />
          {microphone !== 'granted' && microphone !== 'notApplicable' && microphone !== 'loading' && (
            <Btn variant="ghost" size="sm" onClick={reRequestMicrophone}>
              {microphone === 'denied' || microphone === 'restricted' ? t('settings.permissions.openSystem') : t('settings.permissions.grant')}
            </Btn>
          )}
        </div>
      </SettingRow>
      {capability?.requiresAccessibilityPermission && platformCaps?.platform !== 'android' && (
        <SettingRow label={t('settings.permissions.accLabel')}>
          <div style={{ display: 'flex', gap: 8, alignItems: 'center', justifyContent: 'flex-end', width: '100%' }}>
            <PermissionPill status={accessibility} />
            {accessibility !== 'granted' && accessibility !== 'notApplicable' && (
              <Btn variant="ghost" size="sm" onClick={reRequestAccessibility}>
                {t('settings.permissions.grant')}
              </Btn>
            )}
          </div>
        </SettingRow>
      )}
      {platformCaps?.supportsDesktopHotkey === true && (
      <SettingRow label={t('settings.permissions.hotkeyLabel')}>
        <div style={{ display: 'flex', gap: 8, alignItems: 'center', minWidth: 0, justifyContent: 'flex-end', width: '100%' }}>
          {hotkey?.message && (
            <span style={{
              fontSize: 11.5, color: 'var(--ol-ink-4)',
              whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis',
              minWidth: 0, flex: '0 1 auto',
            }}>
              {hotkey.message}
            </span>
          )}
          <HotkeyStatusPill status={hotkey} />
        </div>
      </SettingRow>
      )}
      {platformCaps?.supportsImeInput && platformCaps.platform === 'android' && (
        <SettingRow label={t('settings.permissions.androidImeLabel')}>
          <Pill tone="default">{t('settings.permissions.androidImePlaceholder')}</Pill>
        </SettingRow>
      )}
      {platformCaps?.supportsOverlay && platformCaps.platform === 'android' && (
        <SettingRow label={t('settings.permissions.androidOverlayLabel')}>
          <Pill tone="default">{t('settings.permissions.androidOverlayPlaceholder')}</Pill>
        </SettingRow>
      )}
      {windowsIme?.state !== 'notWindows' && platformCaps?.platform !== 'android' && (
        <SettingRow label={t('settings.permissions.windowsImeLabel')}>
          <div style={{ display: 'flex', gap: 8, alignItems: 'center', minWidth: 0, justifyContent: 'flex-end', width: '100%' }}>
            {windowsIme && (
              <span style={{
                fontSize: 11.5, color: 'var(--ol-ink-4)',
                whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis',
                minWidth: 0, flex: '0 1 auto',
              }}>
                {t(`settings.permissions.windowsIme.${windowsIme.state}`)}
              </span>
            )}
            <WindowsImeStatusPill status={windowsIme} />
          </div>
        </SettingRow>
      )}
      <SettingRow label={t('settings.permissions.networkLabel')}>
        <div style={{ display: 'flex', gap: 8, alignItems: 'center', justifyContent: 'flex-end', width: '100%' }}>
          {network && network.latencyMs != null && (
            <span style={{ fontSize: 11, color: 'var(--ol-ink-4)' }}>
              {network.latencyMs}ms
            </span>
          )}
          <NetworkStatusPill status={network} />
          {network && !network.online && (
            <Btn variant="ghost" size="sm" onClick={refreshNetwork}>
              {t('common.retry') ?? '重试'}
            </Btn>
          )}
        </div>
      </SettingRow>
    </Card>
  );
}

function PermissionPill({ status }: { status: PermissionStatus | 'loading' }) {
  const { t } = useTranslation();
  if (status === 'loading') {
    return <Pill tone="default">{t('settings.permissions.checking')}</Pill>;
  }
  if (status === 'granted') {
    return <Pill tone="ok"><Icon name="check" size={11} />{t('settings.permissions.granted')}</Pill>;
  }
  if (status === 'notApplicable') {
    return <Pill tone="default">{t('settings.permissions.notApplicable')}</Pill>;
  }
  if (status === 'denied' || status === 'restricted') {
    return <Pill tone="outline">{t('settings.permissions.denied')}</Pill>;
  }
  return <Pill tone="outline">{t('settings.permissions.indeterminate')}</Pill>;
}

function HotkeyStatusPill({ status }: { status: HotkeyStatus | null }) {
  const { t } = useTranslation();
  if (!status) {
    return <Pill tone="default">{t('settings.permissions.checking')}</Pill>;
  }
  if (status.state === 'installed') {
    return <Pill tone="ok"><Icon name="check" size={11} />{t('settings.permissions.hotkeyInstalled')}</Pill>;
  }
  if (status.state === 'starting') {
    return <Pill tone="default">{t('settings.permissions.hotkeyStarting')}</Pill>;
  }
  return <Pill tone="outline">{t('settings.permissions.hotkeyFailed')}</Pill>;
}

function WindowsImeStatusPill({ status }: { status: WindowsImeStatus | null }) {
  const { t } = useTranslation();
  if (!status) {
    return <Pill tone="default">{t('settings.permissions.checking')}</Pill>;
  }
  if (status.state === 'installed') {
    return <Pill tone="ok"><Icon name="check" size={11} />{t('settings.permissions.windowsImeInstalled')}</Pill>;
  }
  return <Pill tone="outline">{t('settings.permissions.windowsImeUnavailable')}</Pill>;
}

function NetworkStatusPill({ status }: { status: NetworkCheckResult | null }) {
  const { t } = useTranslation();
  if (!status) {
    return <Pill tone="default">{t('settings.permissions.checking')}</Pill>;
  }
  if (status.online) {
    return <Pill tone="ok"><Icon name="check" size={11} />{t('settings.permissions.networkOk')}</Pill>;
  }
  return <Pill tone="outline">{t('settings.permissions.networkOffline') ?? '不可用'}</Pill>;
}
