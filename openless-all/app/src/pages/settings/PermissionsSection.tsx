// 权限/连通性面板：麦克风 / 辅助功能 / 全局热键 / Windows IME / 网络。
// 内含三个状态 Pill + 适配器名称翻译辅助函数。

import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { AndroidPermissionsPanel } from '@android/components/AndroidPermissionsPanel';
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
import { checkAndroidMicrophoneAccess, requestAndroidMicrophoneAccess } from '@android/lib/androidMicrophonePermission';
import type {
  HotkeyStatus,
  PermissionStatus,
  PlatformCapabilities,
  WindowsImeStatus,
} from '../../lib/types';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { Btn, Card, Pill } from '../_atoms';
import { SettingRow } from './shared';

const ANDROID_SETUP_WIZARD_COMPLETE_KEY = 'openless.androidSetupWizardComplete';

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
      platformCaps?.platform === 'android'
        ? checkAndroidMicrophoneAccess()
        : checkMicrophonePermission(),
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
    // issue #470：热键状态改为纯事件驱动，去掉每秒轮询，靠下方 focus/visibilitychange 刷新。
    // 麦克风检查会短暂打开输入流，避免每秒探测导致隐私指示器频繁闪烁。
    const permissionId = window.setInterval(refreshPermissions, 10000);
    const networkId = window.setInterval(refreshNetwork, 30000);
    const refreshAll = () => {
      refreshPermissions();
      if (platformCaps?.supportsDesktopHotkey === true) {
        refreshHotkey();
      }
      if (platformCaps?.platform !== 'android') {
        refreshWindowsIme();
      }
      refreshNetwork();
    };
    const onFocus = () => refreshAll();
    const onVisibility = () => { if (document.visibilityState === 'visible') refreshAll(); };
    window.addEventListener('focus', onFocus);
    document.addEventListener('visibilitychange', onVisibility);
    return () => {
      window.clearInterval(permissionId);
      window.clearInterval(networkId);
      window.removeEventListener('focus', onFocus);
      document.removeEventListener('visibilitychange', onVisibility);
    };
  }, [platformCaps?.platform, platformCaps?.supportsDesktopHotkey]);

  const reRequestAccessibility = async () => {
    await requestAccessibilityPermission();
    refreshPermissions();
  };

  const reRequestMicrophone = async () => {
    const isAndroid = platformCaps?.platform === 'android';
    if (isAndroid && (microphone === 'denied' || microphone === 'restricted')) {
      await openSystemSettings('microphone');
      await refreshPermissions();
      return;
    }
    const status = isAndroid
      ? await requestAndroidMicrophoneAccess()
      : await requestMicrophonePermission();
    setMicrophone(status);
    if (!isAndroid && (status === 'denied' || status === 'restricted')) {
      await openSystemSettings('microphone');
    }
    await refreshPermissions();
  };

  return (
    <Card>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 10, marginBottom: 6 }}>
        <div style={{ fontSize: 13, fontWeight: 600 }}>{t('settings.permissions.title')}</div>
        {platformCaps?.platform === 'android' && (
          <Btn
            variant="ghost"
            size="sm"
            onClick={() => {
              localStorage.removeItem(ANDROID_SETUP_WIZARD_COMPLETE_KEY);
              window.location.reload();
            }}
          >
            {t('settings.permissions.rerunAndroidSetup')}
          </Btn>
        )}
      </div>
      <SettingRow label={t('settings.permissions.micLabel')}>
        <div style={{ display: 'flex', gap: 8, alignItems: 'center', justifyContent: 'flex-end', width: '100%', flexWrap: 'wrap', minWidth: 0 }}>
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
          <div style={{ display: 'flex', gap: 8, alignItems: 'center', justifyContent: 'flex-end', width: '100%', flexWrap: 'wrap', minWidth: 0 }}>
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
        <div style={{ display: 'flex', gap: 8, alignItems: 'center', minWidth: 0, justifyContent: 'flex-end', width: '100%', flexWrap: 'wrap' }}>
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
      {platformCaps?.supportsOverlay && platformCaps.platform === 'android' && (
        <AndroidPermissionsPanel />
      )}
      {windowsIme?.state !== 'notWindows' && platformCaps?.platform !== 'android' && (
        <SettingRow label={t('settings.permissions.windowsImeLabel')}>
          <div style={{ display: 'flex', gap: 8, alignItems: 'center', minWidth: 0, justifyContent: 'flex-end', width: '100%', flexWrap: 'wrap' }}>
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
        <div style={{ display: 'flex', gap: 8, alignItems: 'center', justifyContent: 'flex-end', width: '100%', flexWrap: 'wrap', minWidth: 0 }}>
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
