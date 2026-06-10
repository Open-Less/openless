// 权限/连通性面板：麦克风 / 辅助功能 / 全局热键 / Windows IME / 网络。
// 内含三个状态 Pill + 适配器名称翻译辅助函数。

import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../../components/Icon';
import {
  checkAccessibilityPermission,
  checkMicrophonePermission,
  checkNetwork,
  getAndroidAccessibilityStatus,
  getAndroidOverlayStatus,
  getHotkeyStatus,
  getSettings,
  getWindowsImeStatus,
  openSystemSettings,
  requestAccessibilityPermission,
  requestAndroidAccessibilityPermission,
  requestAndroidOverlayPermission,
  requestMicrophonePermission,
  setSettings,
} from '../../lib/ipc';
import type { NetworkCheckResult } from '../../lib/ipc';
import { getPlatformCapabilities } from '../../lib/platform';
import { checkAndroidMicrophoneAccess, requestAndroidMicrophoneAccess } from '../../lib/androidMicrophonePermission';
import type {
  AndroidAccessibilityStatus,
  AndroidOverlayActivationMode,
  AndroidOverlayLeftSwipeAction,
  AndroidInsertStrategy,
  AndroidOverlayStatus,
  AndroidOverlayTrigger,
  HotkeyStatus,
  PermissionStatus,
  PlatformCapabilities,
  UserPreferences,
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
  const [androidOverlay, setAndroidOverlay] = useState<AndroidOverlayStatus | null>(null);
  const [androidAccessibility, setAndroidAccessibility] = useState<AndroidAccessibilityStatus | null>(null);
  const [androidPrefs, setAndroidPrefs] = useState<Pick<UserPreferences, AndroidPreferenceKey> | null>(null);
  const { capability } = useHotkeySettings();

  useEffect(() => {
    void getPlatformCapabilities().then(setPlatformCaps);
  }, []);

  const refreshAndroid = async () => {
    if (platformCaps?.platform !== 'android') return;
    const [overlay, accessibility, settings] = await Promise.all([
      getAndroidOverlayStatus(),
      getAndroidAccessibilityStatus(),
      getSettings(),
    ]);
    let migratedSettings = settings;
    if (settings.androidOverlayTrigger === 'keyboard') {
      migratedSettings = {
        ...settings,
        androidOverlayTrigger: normalizeAndroidOverlayTrigger(settings.androidOverlayTrigger),
      };
      await setSettings(migratedSettings);
    }
    setAndroidOverlay(overlay);
    setAndroidAccessibility(accessibility);
    setAndroidPrefs({
      androidInsertStrategy: migratedSettings.androidInsertStrategy,
      androidOverlayTrigger: migratedSettings.androidOverlayTrigger,
      androidOverlayActivationMode: migratedSettings.androidOverlayActivationMode,
      androidOverlayLeftSwipeAction: migratedSettings.androidOverlayLeftSwipeAction,
      androidOverlaySizeDp: migratedSettings.androidOverlaySizeDp,
    });
  };

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
    } else {
      refreshAndroid();
    }
    refreshNetwork();
    const hotkeyId = platformCaps?.supportsDesktopHotkey === true
      ? window.setInterval(refreshHotkey, 1000)
      : undefined;
    // 麦克风检查会短暂打开输入流，避免每秒探测导致隐私指示器频繁闪烁。
    const permissionId = window.setInterval(refreshPermissions, 10000);
    const androidId = platformCaps?.platform === 'android'
      ? window.setInterval(refreshAndroid, 3000)
      : undefined;
    const networkId = window.setInterval(refreshNetwork, 30000);
    const onFocus = () => {
      refreshPermissions();
      if (platformCaps?.supportsDesktopHotkey === true) {
        refreshHotkey();
      }
      if (platformCaps?.platform !== 'android') {
        refreshWindowsIme();
      } else {
        refreshAndroid();
      }
      refreshNetwork();
    };
    window.addEventListener('focus', onFocus);
    return () => {
      if (hotkeyId !== undefined) window.clearInterval(hotkeyId);
      window.clearInterval(permissionId);
      if (androidId !== undefined) window.clearInterval(androidId);
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
    const status = platformCaps?.platform === 'android'
      ? await requestAndroidMicrophoneAccess()
      : await requestMicrophonePermission();
    setMicrophone(status);
    if (status === 'denied' || status === 'restricted') {
      await openSystemSettings('microphone');
    }
    refreshPermissions();
  };

  const updateAndroidPref = async <K extends AndroidPreferenceKey>(key: K, value: UserPreferences[K]) => {
    const settings = await getSettings();
    const nextValue = key === 'androidOverlayTrigger'
      ? normalizeAndroidOverlayTrigger(value as AndroidOverlayTrigger)
      : value;
    const next = {
      ...settings,
      [key]: nextValue,
    };
    await setSettings(next);
    setAndroidPrefs({
      androidInsertStrategy: next.androidInsertStrategy,
      androidOverlayTrigger: next.androidOverlayTrigger,
      androidOverlayActivationMode: next.androidOverlayActivationMode,
      androidOverlayLeftSwipeAction: next.androidOverlayLeftSwipeAction,
      androidOverlaySizeDp: next.androidOverlaySizeDp,
    });
    await refreshAndroid();
  };

  return (
    <Card>
      <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 6 }}>{t('settings.permissions.title')}</div>
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
        <>
          <SettingRow label={t('settings.permissions.androidOverlayLabel')}>
            <div style={{ display: 'flex', gap: 8, alignItems: 'center', justifyContent: 'flex-end', width: '100%', flexWrap: 'wrap', minWidth: 0 }}>
              {androidOverlay?.message && (
                <span style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', maxWidth: 220, textAlign: 'right' }}>
                  {androidOverlay.message}
                </span>
              )}
              <AndroidOverlayStatusPill status={androidOverlay} />
              {androidOverlay?.permission !== 'granted' && (
                <Btn variant="ghost" size="sm" onClick={() => { void requestAndroidOverlayPermission().then(refreshAndroid); }}>
                  {t('settings.permissions.grant')}
                </Btn>
              )}
            </div>
          </SettingRow>
          <SettingRow label={t('settings.permissions.androidAccessibilityLabel')}>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 6, alignItems: 'flex-end', width: '100%', minWidth: 0 }}>
              <div style={{ display: 'flex', gap: 8, alignItems: 'center', justifyContent: 'flex-end', width: '100%', flexWrap: 'wrap', minWidth: 0 }}>
                {androidAccessibility?.message && (
                  <span style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', maxWidth: 220, textAlign: 'right' }}>
                    {androidAccessibility.message}
                  </span>
                )}
                <AndroidAccessibilityStatusPill status={androidAccessibility} />
                {!androidAccessibility?.enabled && (
                  <Btn variant="ghost" size="sm" onClick={() => { void requestAndroidAccessibilityPermission().then(refreshAndroid); }}>
                    {t('settings.permissions.openSystem')}
                  </Btn>
                )}
              </div>
              <span style={{ fontSize: 11, color: 'var(--ol-ink-4)', maxWidth: 300, textAlign: 'right' }}>
                {t('settings.permissions.androidAccessibilityImpact')}
              </span>
            </div>
          </SettingRow>
          <SettingRow label={t('settings.permissions.androidInsertStrategyLabel')}>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 6, alignItems: 'flex-end', width: '100%' }}>
              <select
                value={androidPrefs?.androidInsertStrategy ?? 'accessibility'}
                onChange={(event) => { void updateAndroidPref('androidInsertStrategy', event.target.value as AndroidInsertStrategy); }}
                style={{ minWidth: 180, maxWidth: '100%' }}
              >
                <option value="accessibility">{t('settings.permissions.androidInsertStrategy.accessibility')}</option>
                <option value="clipboard">{t('settings.permissions.androidInsertStrategy.clipboard')}</option>
              </select>
              <span style={{ fontSize: 11, color: 'var(--ol-ink-4)', textAlign: 'right' }}>
                {t(`settings.permissions.androidInsertStrategyHint.${androidPrefs?.androidInsertStrategy ?? 'accessibility'}`)}
              </span>
            </div>
          </SettingRow>
          <SettingRow label={t('settings.permissions.androidOverlayTriggerLabel')}>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 6, alignItems: 'flex-end', width: '100%' }}>
              <select
                value={androidPrefs?.androidOverlayTrigger ?? 'background'}
                onChange={(event) => { void updateAndroidPref('androidOverlayTrigger', event.target.value as AndroidOverlayTrigger); }}
                style={{ minWidth: 180, maxWidth: '100%' }}
              >
                <option value="background">{t('settings.permissions.androidOverlayTrigger.background')}</option>
                <option value="keyboard" disabled>{t('settings.permissions.androidOverlayTrigger.keyboard')}</option>
                <option value="always">{t('settings.permissions.androidOverlayTrigger.always')}</option>
              </select>
              <span style={{ fontSize: 11, color: 'var(--ol-ink-4)', textAlign: 'right' }}>
                {t(`settings.permissions.androidOverlayTriggerHint.${androidPrefs?.androidOverlayTrigger ?? 'background'}`)}
              </span>
              <span style={{ fontSize: 11, color: 'var(--ol-ink-4)', textAlign: 'right' }}>
                {t('settings.permissions.androidOverlayTriggerDisabled.keyboard')}
              </span>
            </div>
          </SettingRow>
          <SettingRow label={t('settings.permissions.androidOverlayActivationModeLabel')}>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 6, alignItems: 'flex-end', width: '100%' }}>
              <select
                value={androidPrefs?.androidOverlayActivationMode ?? 'tap'}
                onChange={(event) => { void updateAndroidPref('androidOverlayActivationMode', event.target.value as AndroidOverlayActivationMode); }}
                style={{ minWidth: 180, maxWidth: '100%' }}
              >
                <option value="tap">{t('settings.permissions.androidOverlayActivationMode.tap')}</option>
                <option value="long_press">{t('settings.permissions.androidOverlayActivationMode.long_press')}</option>
              </select>
              <span style={{ fontSize: 11, color: 'var(--ol-ink-4)', textAlign: 'right' }}>
                {t(`settings.permissions.androidOverlayActivationModeHint.${androidPrefs?.androidOverlayActivationMode ?? 'tap'}`)}
              </span>
            </div>
          </SettingRow>
          <SettingRow label={t('settings.permissions.androidOverlayLeftSwipeActionLabel')}>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 6, alignItems: 'flex-end', width: '100%' }}>
              <select
                value={androidPrefs?.androidOverlayLeftSwipeAction ?? 'translation'}
                onChange={(event) => { void updateAndroidPref('androidOverlayLeftSwipeAction', event.target.value as AndroidOverlayLeftSwipeAction); }}
                style={{ minWidth: 180, maxWidth: '100%' }}
              >
                <option value="translation">{t('settings.permissions.androidOverlayLeftSwipeAction.translation')}</option>
                <option value="style_pack">{t('settings.permissions.androidOverlayLeftSwipeAction.style_pack')}</option>
              </select>
              <span style={{ fontSize: 11, color: 'var(--ol-ink-4)', textAlign: 'right' }}>
                {t(`settings.permissions.androidOverlayLeftSwipeActionHint.${androidPrefs?.androidOverlayLeftSwipeAction ?? 'translation'}`)}
              </span>
            </div>
          </SettingRow>
          <SettingRow label={t('settings.permissions.androidOverlaySizeLabel')}>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 6, alignItems: 'flex-end', width: '100%' }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 10, minWidth: 180, maxWidth: '100%' }}>
                <input
                  type="range"
                  min={48}
                  max={120}
                  step={4}
                  value={androidPrefs?.androidOverlaySizeDp ?? 72}
                  onChange={(event) => {
                    void updateAndroidPref('androidOverlaySizeDp', clampAndroidOverlaySize(Number(event.target.value)));
                  }}
                  style={{ width: 132 }}
                />
                <span style={{ fontSize: 12, color: 'var(--ol-ink-3)', minWidth: 42, textAlign: 'right' }}>
                  {androidPrefs?.androidOverlaySizeDp ?? 72} dp
                </span>
              </div>
              <span style={{ fontSize: 11, color: 'var(--ol-ink-4)', textAlign: 'right' }}>
                {t('settings.permissions.androidOverlaySizeHint')}
              </span>
            </div>
          </SettingRow>
        </>
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

type AndroidPreferenceKey =
  | 'androidInsertStrategy'
  | 'androidOverlayTrigger'
  | 'androidOverlayActivationMode'
  | 'androidOverlayLeftSwipeAction'
  | 'androidOverlaySizeDp';

function normalizeAndroidOverlayTrigger(trigger: AndroidOverlayTrigger): AndroidOverlayTrigger {
  return trigger === 'keyboard' ? 'background' : trigger;
}

function clampAndroidOverlaySize(size: number): number {
  if (!Number.isFinite(size)) return 72;
  return Math.min(120, Math.max(48, Math.round(size / 4) * 4));
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

function AndroidOverlayStatusPill({ status }: { status: AndroidOverlayStatus | null }) {
  const { t } = useTranslation();
  if (!status) return <Pill tone="default">{t('settings.permissions.checking')}</Pill>;
  if (status.permission === 'granted') {
    return <Pill tone="ok"><Icon name="check" size={11} />{t('settings.permissions.granted')}</Pill>;
  }
  return <Pill tone="outline">{t('settings.permissions.denied')}</Pill>;
}

function AndroidAccessibilityStatusPill({ status }: { status: AndroidAccessibilityStatus | null }) {
  const { t } = useTranslation();
  if (!status) return <Pill tone="default">{t('settings.permissions.checking')}</Pill>;
  if (status.enabled) {
    return <Pill tone="ok"><Icon name="check" size={11} />{t('settings.permissions.granted')}</Pill>;
  }
  return <Pill tone="outline">{t('settings.permissions.denied')}</Pill>;
}
