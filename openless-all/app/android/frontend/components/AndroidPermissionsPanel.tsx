import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { TFunction } from 'i18next';
import { Icon } from '../../../src/components/Icon';
import { getSettings, setSettings } from '../../../src/lib/ipc/settings';
import type { UserPreferences } from '../../../src/lib/types';
import { Btn, Pill } from '../../../src/pages/_atoms';
import { SettingRow } from '../../../src/pages/settings/shared';
import {
  getAndroidAccessibilityStatus,
  getAndroidOverlayStatus,
  getAndroidShizukuStatus,
  openShizukuApp,
  recoverAndroidAccessibility,
  requestAndroidAccessibilityPermission,
  requestAndroidOverlayPermission,
  requestAndroidShizukuPermission,
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
  AndroidShizukuStatus,
} from '../lib/androidTypes';
import {
  clampAndroidOverlaySize,
  normalizeAndroidOverlayTrigger,
} from '../lib/androidTypes';

type AndroidPrefsSlice = Pick<UserPreferences, AndroidPreferenceKey>;

function pickAndroidPrefs(settings: UserPreferences): AndroidPrefsSlice {
  return {
    androidInsertStrategy: settings.androidInsertStrategy,
    androidOverlayTrigger: settings.androidOverlayTrigger,
    androidOverlayActivationMode: settings.androidOverlayActivationMode,
    androidOverlayLeftSwipeAction: settings.androidOverlayLeftSwipeAction,
    androidOverlayCancelSwipeDirection: settings.androidOverlayCancelSwipeDirection,
    androidOverlaySizeDp: settings.androidOverlaySizeDp,
  };
}

let androidOverlaySaveQueue = Promise.resolve();

function enqueueAndroidOverlaySave<T>(task: () => Promise<T>): Promise<T> {
  const run = androidOverlaySaveQueue.then(task);
  androidOverlaySaveQueue = run.then(() => undefined, () => undefined);
  return run;
}

function persistAndroidOverlayPrefs(patch: Partial<UserPreferences>): Promise<AndroidPrefsSlice> {
  return enqueueAndroidOverlaySave(async () => {
    const settings = await getSettings();
    const next = {
      ...settings,
      ...patch,
    };
    await setSettings(next);
    return pickAndroidPrefs(next);
  });
}

type AndroidPermissionsPanelMode = 'all' | 'accessibility' | 'overlayPermission' | 'overlayConfig';

interface AndroidPermissionsPanelProps {
  mode?: AndroidPermissionsPanelMode;
}

export function AndroidPermissionsPanel({ mode = 'all' }: AndroidPermissionsPanelProps) {
  const { t } = useTranslation();
  const [androidOverlay, setAndroidOverlay] = useState<AndroidOverlayStatus | null>(null);
  const [androidAccessibility, setAndroidAccessibility] = useState<AndroidAccessibilityStatus | null>(null);
  const [androidShizuku, setAndroidShizuku] = useState<AndroidShizukuStatus | null>(null);
  const [shizukuRecoveryMessageKey, setShizukuRecoveryMessageKey] = useState<string | null>(null);
  const [shizukuActionMessageKey, setShizukuActionMessageKey] = useState<string | null>(null);
  const [shizukuRecoveryPending, setShizukuRecoveryPending] = useState(false);
  const [androidPrefs, setAndroidPrefs] = useState<Pick<UserPreferences, AndroidPreferenceKey> | null>(null);
  const [sizeDraft, setSizeDraft] = useState<number | null>(null);
  const sizeDebounceRef = useRef<number | null>(null);
  const sizePendingRef = useRef(false);

  const refreshAndroid = async () => {
    const [overlayResult, accessibilityResult, shizukuResult, settingsResult] =
      await Promise.allSettled([
        getAndroidOverlayStatus(),
        getAndroidAccessibilityStatus(),
        getAndroidShizukuStatus(),
        getSettings(),
      ]);
    if (overlayResult.status === 'fulfilled') {
      setAndroidOverlay(overlayResult.value);
    }
    if (accessibilityResult.status === 'fulfilled') {
      setAndroidAccessibility(accessibilityResult.value);
    }
    if (shizukuResult.status === 'fulfilled') {
      const shizuku = shizukuResult.value;
      setAndroidShizuku((prev) => {
        const operational = shizuku.accessibility.operational;
        if (operational && !prev?.accessibility.operational) {
          setShizukuRecoveryMessageKey(null);
        }
        if (shizuku.lastPermissionMessageKey) {
          setShizukuActionMessageKey(null);
        }
        return shizuku;
      });
    }
    if (settingsResult.status === 'fulfilled') {
      let migratedSettings = settingsResult.value;
      if (migratedSettings.androidOverlayTrigger === 'keyboard') {
        const migratedPrefs = await persistAndroidOverlayPrefs({
          androidOverlayTrigger: normalizeAndroidOverlayTrigger(
            migratedSettings.androidOverlayTrigger,
          ),
        });
        migratedSettings = { ...migratedSettings, ...migratedPrefs };
      }
      setAndroidPrefs(pickAndroidPrefs(migratedSettings));
    }
  };

  const handleRecoverAccessibility = async () => {
    if (shizukuRecoveryPending) return;
    const confirmed = window.confirm(t('settings.permissions.androidShizukuRecoverConfirm'));
    if (!confirmed) return;
    setShizukuActionMessageKey(null);
    setShizukuRecoveryPending(true);
    try {
      const result = await recoverAndroidAccessibility(true);
      setShizukuRecoveryMessageKey(result.messageKey);
      await refreshAndroid();
    } finally {
      setShizukuRecoveryPending(false);
    }
  };

  const shizukuDisplayMessageKey = androidShizuku?.lastPermissionMessageKey
    ?? shizukuRecoveryMessageKey
    ?? shizukuActionMessageKey
    ?? androidShizuku?.messageKey
    ?? null;

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

  useEffect(() => {
    if (sizePendingRef.current) return;
    if (androidPrefs?.androidOverlaySizeDp != null) {
      setSizeDraft(androidPrefs.androidOverlaySizeDp);
    }
  }, [androidPrefs?.androidOverlaySizeDp]);

  useEffect(() => {
    return () => {
      if (sizeDebounceRef.current) clearTimeout(sizeDebounceRef.current);
    };
  }, []);

  const saveAndroidOverlaySize = async (value: number) => {
    const clamped = clampAndroidOverlaySize(value);
    try {
      const nextPrefs = await persistAndroidOverlayPrefs({ androidOverlaySizeDp: clamped });
      setAndroidPrefs((prev) => (prev ? { ...prev, ...nextPrefs } : nextPrefs));
      setSizeDraft(clamped);
    } catch (error) {
      console.error('[android] failed to save overlay size', error);
      const settings = await getSettings();
      const rolledBack = settings.androidOverlaySizeDp;
      const safeDraft = rolledBack != null ? clampAndroidOverlaySize(rolledBack) : null;
      setAndroidPrefs((prev) => (prev ? { ...prev, androidOverlaySizeDp: rolledBack } : null));
      setSizeDraft(safeDraft);
    } finally {
      sizePendingRef.current = false;
    }
  };

  const scheduleAndroidOverlaySizeSave = (value: number) => {
    if (sizeDebounceRef.current) clearTimeout(sizeDebounceRef.current);
    sizeDebounceRef.current = window.setTimeout(() => {
      sizeDebounceRef.current = null;
      void saveAndroidOverlaySize(value);
    }, 200);
  };

  const flushAndroidOverlaySizeSave = (value: number) => {
    const hasDebounce = sizeDebounceRef.current != null;
    const hasPending = sizePendingRef.current;
    if (!hasDebounce && !hasPending) return;

    if (sizeDebounceRef.current) {
      clearTimeout(sizeDebounceRef.current);
      sizeDebounceRef.current = null;
    }

    const clamped = clampAndroidOverlaySize(value);
    const committed = androidPrefs?.androidOverlaySizeDp;
    if (committed != null && clamped === committed) {
      sizePendingRef.current = false;
      return;
    }

    void saveAndroidOverlaySize(clamped);
  };

  const handleAndroidOverlaySizeChange = (value: number) => {
    const clamped = clampAndroidOverlaySize(value);
    sizePendingRef.current = true;
    setSizeDraft(clamped);
    scheduleAndroidOverlaySizeSave(clamped);
  };

  const updateAndroidPref = async <K extends AndroidPreferenceKey>(key: K, value: UserPreferences[K]) => {
    if (key !== 'androidOverlaySizeDp' && sizeDebounceRef.current) {
      clearTimeout(sizeDebounceRef.current);
      sizeDebounceRef.current = null;
    }

    const patch: Partial<UserPreferences> = {
      [key]: key === 'androidOverlayTrigger'
        ? normalizeAndroidOverlayTrigger(value as AndroidOverlayTrigger)
        : value,
    };

    if (key !== 'androidOverlaySizeDp') {
      const committedSize = androidPrefs?.androidOverlaySizeDp;
      const draftSize = sizeDraft;
      if (sizePendingRef.current || (draftSize != null && draftSize !== committedSize)) {
        patch.androidOverlaySizeDp = clampAndroidOverlaySize(draftSize ?? committedSize ?? 72);
      }
      sizePendingRef.current = false;
    }

    try {
      const nextPrefs = await persistAndroidOverlayPrefs(patch);
      setAndroidPrefs(nextPrefs);
      if (patch.androidOverlaySizeDp != null) {
        setSizeDraft(nextPrefs.androidOverlaySizeDp);
      }
      await refreshAndroid();
    } catch (error) {
      console.error('[android] failed to save overlay pref', error);
      await refreshAndroid();
    }
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
            {resolveAccessibilityMessage(t, androidAccessibility?.messageKey) && (
              <span style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', maxWidth: 220, textAlign: 'right' }}>
                {resolveAccessibilityMessage(t, androidAccessibility?.messageKey)}
              </span>
            )}
            <AndroidAccessibilityStatusPill status={androidAccessibility} />
            {(!androidAccessibility?.enabled || androidAccessibility?.operational === false) && (
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
      {showAccessibility && (
      <SettingRow label={t('settings.permissions.androidShizukuLabel')}>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 6, alignItems: 'flex-end', width: '100%', minWidth: 0 }}>
          <div style={{ display: 'flex', gap: 8, alignItems: 'center', justifyContent: 'flex-end', width: '100%', flexWrap: 'wrap', minWidth: 0 }}>
            {resolveShizukuMessage(t, shizukuDisplayMessageKey) && (
              <span style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', maxWidth: 220, textAlign: 'right' }}>
                {resolveShizukuMessage(t, shizukuDisplayMessageKey)}
              </span>
            )}
            <AndroidShizukuStatusPill status={androidShizuku} />
            {(androidShizuku?.state === 'notInstalled' || androidShizuku?.state === 'notRunning' || androidShizuku?.state === 'binderDead') && (
              <Btn variant="ghost" size="sm" onClick={() => {
                setShizukuRecoveryMessageKey(null);
                setShizukuActionMessageKey(null);
                void openShizukuApp().then((result) => {
                  if (!result.launched) {
                    setShizukuActionMessageKey(result.messageKey);
                  }
                  return refreshAndroid();
                });
              }}>
                {t('settings.permissions.androidShizukuOpenApp')}
              </Btn>
            )}
            {androidShizuku?.state === 'notAuthorized' && (
              <Btn variant="ghost" size="sm" onClick={() => {
                setShizukuRecoveryMessageKey(null);
                setShizukuActionMessageKey(null);
                void requestAndroidShizukuPermission().then((result) => {
                  if (!result.launched) {
                    setShizukuActionMessageKey(result.messageKey);
                  }
                  return refreshAndroid();
                });
              }}>
                {t('settings.permissions.androidShizukuRequestPermission')}
              </Btn>
            )}
            {androidShizuku?.state === 'authorized'
              && !androidShizuku.accessibility.operational && (
              <Btn
                variant="ghost"
                size="sm"
                disabled={shizukuRecoveryPending}
                onClick={() => { void handleRecoverAccessibility(); }}
              >
                {shizukuRecoveryPending
                  ? t('settings.permissions.checking')
                  : t('settings.permissions.androidShizukuRecover')}
              </Btn>
            )}
            {shizukuRecoveryMessageKey
              && androidShizuku?.state === 'authorized'
              && !androidShizuku.accessibility.operational
              && (shizukuRecoveryMessageKey === 'partial_rollback'
                || shizukuRecoveryMessageKey === 'manual_required'
                || shizukuRecoveryMessageKey === 'oem_rollback'
                || shizukuRecoveryMessageKey === 'concurrent_change') && (
              <Btn variant="ghost" size="sm" onClick={() => { void requestAndroidAccessibilityPermission().then(refreshAndroid); }}>
                {t('settings.permissions.openSystem')}
              </Btn>
            )}
          </div>
          {androidShizuku?.state === 'authorized' && (
            <span style={{ fontSize: 11, color: 'var(--ol-ink-4)', maxWidth: 300, textAlign: 'right' }}>
              {androidShizuku.accessibility.operational
                ? t('settings.permissions.androidShizukuAccessibilityOperational')
                : t('settings.permissions.androidShizukuAccessibilityRegistered', {
                    registered: androidShizuku.accessibility.registered
                      ? t('settings.permissions.androidShizukuYes')
                      : t('settings.permissions.androidShizukuNo'),
                    operational: androidShizuku.accessibility.operational
                      ? t('settings.permissions.androidShizukuYes')
                      : t('settings.permissions.androidShizukuNo'),
                  })}
            </span>
          )}
          <span style={{ fontSize: 11, color: 'var(--ol-ink-4)', maxWidth: 300, textAlign: 'right' }}>
            {t('settings.permissions.androidShizukuHint')}
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
              value={sizeDraft ?? androidPrefs?.androidOverlaySizeDp ?? 72}
              onChange={(event) => {
                handleAndroidOverlaySizeChange(Number(event.target.value));
              }}
              onPointerUp={(event) => {
                flushAndroidOverlaySizeSave(Number(event.currentTarget.value));
              }}
              onTouchEnd={(event) => {
                flushAndroidOverlaySizeSave(Number(event.currentTarget.value));
              }}
              onBlur={(event) => {
                flushAndroidOverlaySizeSave(Number(event.currentTarget.value));
              }}
              style={{ width: 132 }}
            />
            <span style={{ fontSize: 12, color: 'var(--ol-ink-3)', minWidth: 42, textAlign: 'right' }}>
              {sizeDraft ?? androidPrefs?.androidOverlaySizeDp ?? 72} dp
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
  if (status.enabled && status.operational === false) {
    return <Pill tone="outline">{t('settings.permissions.androidAccessibilityGrantedStale')}</Pill>;
  }
  if (status.enabled) {
    return <Pill tone="ok"><Icon name="check" size={11} />{t('settings.permissions.granted')}</Pill>;
  }
  return <Pill tone="outline">{t('settings.permissions.denied')}</Pill>;
}

function resolveShizukuMessage(t: TFunction, key: string | null | undefined): string {
  if (!key) return '';
  return t(`settings.permissions.androidShizukuMessages.${key}`, { defaultValue: key });
}

function resolveAccessibilityMessage(t: TFunction, key: string | null | undefined): string {
  if (!key) return '';
  return t(`settings.permissions.androidAccessibilityMessages.${key}`, { defaultValue: '' });
}

function AndroidShizukuStatusPill({ status }: { status: AndroidShizukuStatus | null }) {
  const { t } = useTranslation();
  if (!status) return <Pill tone="default">{t('settings.permissions.checking')}</Pill>;
  const labelKey = `settings.permissions.androidShizukuState.${status.state}` as const;
  if (status.state === 'authorized') {
    return <Pill tone="ok"><Icon name="check" size={11} />{t(labelKey)}</Pill>;
  }
  if (status.state === 'notAndroid') {
    return <Pill tone="default">{t(labelKey)}</Pill>;
  }
  return <Pill tone="outline">{t(labelKey)}</Pill>;
}
