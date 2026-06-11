import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../../../src/components/Icon';
import { getSettings, setSettings } from '../../../src/lib/ipc';
import type { UserPreferences } from '../../../src/lib/types';
import { Btn, Pill } from '../../../src/pages/_atoms';
import { SettingRow } from '../../../src/pages/settings/shared';
import {
  getAndroidAccessibilityStatus,
  getAndroidOverlayStatus,
  requestAndroidAccessibilityPermission,
  requestAndroidOverlayPermission,
} from '../lib/androidIpc';
import type {
  AndroidAccessibilityStatus,
  AndroidInsertStrategy,
  AndroidOverlayActivationMode,
  AndroidOverlayCancelSwipeDirection,
  AndroidOverlayLeftSwipeAction,
  AndroidOverlayStatus,
  AndroidOverlayTrigger,
  AndroidPreferenceKey,
} from '../lib/androidTypes';
import {
  clampAndroidOverlaySize,
  normalizeAndroidOverlayTrigger,
} from '../lib/androidTypes';

type AndroidPermissionsPanelMode = 'all' | 'accessibility' | 'overlayPermission' | 'overlayConfig';

interface AndroidPermissionsPanelProps {
  mode?: AndroidPermissionsPanelMode;
}

export function AndroidPermissionsPanel({ mode = 'all' }: AndroidPermissionsPanelProps) {
  const { t } = useTranslation();
  const [androidOverlay, setAndroidOverlay] = useState<AndroidOverlayStatus | null>(null);
  const [androidAccessibility, setAndroidAccessibility] = useState<AndroidAccessibilityStatus | null>(null);
  const [androidPrefs, setAndroidPrefs] = useState<Pick<UserPreferences, AndroidPreferenceKey> | null>(null);

  const refreshAndroid = async () => {
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
      androidOverlayCancelSwipeDirection: migratedSettings.androidOverlayCancelSwipeDirection,
      androidOverlaySizeDp: migratedSettings.androidOverlaySizeDp,
    });
  };

  useEffect(() => {
    void refreshAndroid();
    const androidId = window.setInterval(refreshAndroid, 3000);
    const onFocus = () => { void refreshAndroid(); };
    window.addEventListener('focus', onFocus);
    return () => {
      window.clearInterval(androidId);
      window.removeEventListener('focus', onFocus);
    };
  }, []);

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
      androidOverlayCancelSwipeDirection: next.androidOverlayCancelSwipeDirection,
      androidOverlaySizeDp: next.androidOverlaySizeDp,
    });
    await refreshAndroid();
  };

  const showOverlayPermission = mode === 'all' || mode === 'overlayPermission';
  const showAccessibility = mode === 'all' || mode === 'accessibility';
  const showOverlayConfig = mode === 'all' || mode === 'overlayConfig';

  return (
    <>
      {showOverlayPermission && (
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
      )}
      {showAccessibility && (
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
      )}
      {showOverlayConfig && (
      <>
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
      <SettingRow label={t('settings.permissions.androidOverlayCancelSwipeDirectionLabel')}>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 6, alignItems: 'flex-end', width: '100%' }}>
          <select
            value={androidPrefs?.androidOverlayCancelSwipeDirection ?? 'up'}
            onChange={(event) => { void updateAndroidPref('androidOverlayCancelSwipeDirection', event.target.value as AndroidOverlayCancelSwipeDirection); }}
            style={{ minWidth: 180, maxWidth: '100%' }}
          >
            <option value="up">{t('settings.permissions.androidOverlayCancelSwipeDirection.up')}</option>
            <option value="down">{t('settings.permissions.androidOverlayCancelSwipeDirection.down')}</option>
          </select>
          <span style={{ fontSize: 11, color: 'var(--ol-ink-4)', textAlign: 'right' }}>
            {t(`settings.permissions.androidOverlayCancelSwipeDirectionHint.${androidPrefs?.androidOverlayCancelSwipeDirection ?? 'up'}`)}
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
    </>
  );
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
