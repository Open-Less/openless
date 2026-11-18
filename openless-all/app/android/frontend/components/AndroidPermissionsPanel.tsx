import { useEffect, useRef, useState, type CSSProperties } from 'react';
import { useTranslation } from 'react-i18next';
import type { TFunction } from 'i18next';
import { Icon } from '../../../src/components/Icon';
import { useHotkeySettings } from '../../../src/data/preferences';
import { useInteraction } from '../../../src/ui/overlays/InteractionProvider';
import type { UserPreferences } from '../../../src/api/contracts';
import { Button as Btn } from '../../../src/ui/controls/Button';
import { Pill } from '../../../src/ui/controls/Pill';
import { SettingRow } from '../../../src/ui/controls/SettingRow';
import { SolidSlider } from '../../../src/ui/controls/Slider';
import { useLayoutStack, useConservativeLayout } from '../../../src/lib/useMobileLayout';
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
  AndroidOverlayGestureAction,
  AndroidOverlayStatus,
  AndroidOverlayTrigger,
  AndroidPreferenceKey,
  AndroidShizukuStatus,
} from '../lib/androidTypes';
import { clampAndroidOverlaySize, normalizeAndroidOverlayTrigger } from '../lib/androidTypes';

type AndroidPermissionsPanelMode =
  'all' | 'permissionsOnly' | 'accessibility' | 'overlayPermission' | 'overlayConfig';

interface AndroidPermissionsPanelProps {
  mode?: AndroidPermissionsPanelMode;
}

export function AndroidPermissionsPanel({ mode = 'all' }: AndroidPermissionsPanelProps) {
  const { t } = useTranslation();
  const { prefs: androidPrefs, updatePrefs } = useHotkeySettings();
  const { confirm } = useInteraction();
  const [actionError, setActionError] = useState(false);
  const reportFailure = () => setActionError(true);
  const baseLayoutStack = useLayoutStack();
  const conservative = useConservativeLayout();
  const layoutStack = baseLayoutStack || conservative;
  const [androidOverlay, setAndroidOverlay] = useState<AndroidOverlayStatus | null>(null);
  const [androidAccessibility, setAndroidAccessibility] =
    useState<AndroidAccessibilityStatus | null>(null);
  const [androidShizuku, setAndroidShizuku] = useState<AndroidShizukuStatus | null>(null);
  const [shizukuRecoveryMessageKey, setShizukuRecoveryMessageKey] = useState<string | null>(null);
  const [shizukuActionMessageKey, setShizukuActionMessageKey] = useState<string | null>(null);
  const [shizukuRecoveryPending, setShizukuRecoveryPending] = useState(false);
  const [sizeDraft, setSizeDraft] = useState<number | null>(null);
  const draftSizeRef = useRef<number | null>(null);
  const draftRevision = useRef(0);
  const sizeDebounceRef = useRef<number | null>(null);
  const recovery = useRef(false);
  const loaded = useRef(false);
  const refreshRevision = useRef(0);
  const migratedLegacyTrigger = useRef(false);

  const refreshAndroid = async () => {
    const request = ++refreshRevision.current;
    const [overlayResult, accessibilityResult, shizukuResult] = await Promise.allSettled([
      getAndroidOverlayStatus(),
      getAndroidAccessibilityStatus(),
      getAndroidShizukuStatus(),
    ]);
    if (!loaded.current || refreshRevision.current !== request) return;
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
  };

  const handleRecoverAccessibility = async () => {
    if (recovery.current) return;
    recovery.current = true;
    setShizukuRecoveryPending(true);
    try {
      const confirmed = await confirm({
        title: t('settings.permissions.androidShizukuRecoverConfirm'),
        tone: 'danger',
      });
      if (!confirmed) return;
      setActionError(false);
      setShizukuActionMessageKey(null);
      const result = await recoverAndroidAccessibility(true);
      if (loaded.current) setShizukuRecoveryMessageKey(result.messageKey);
      await refreshAndroid();
    } catch {
      if (loaded.current) reportFailure();
    } finally {
      recovery.current = false;
      if (loaded.current) setShizukuRecoveryPending(false);
    }
  };

  const requestAndRefresh = async (action: () => Promise<unknown>) => {
    setActionError(false);
    try {
      await action();
      await refreshAndroid();
    } catch {
      if (loaded.current) reportFailure();
    }
  };

  const shizukuDisplayMessageKey =
    androidShizuku?.lastPermissionMessageKey ??
    shizukuRecoveryMessageKey ??
    shizukuActionMessageKey ??
    androidShizuku?.messageKey ??
    null;

  useEffect(() => {
    loaded.current = true;
    void refreshAndroid();
    const androidId = window.setInterval(refreshAndroid, 3000);
    const onFocus = () => {
      void refreshAndroid();
    };
    window.addEventListener('focus', onFocus);
    return () => {
      loaded.current = false;
      refreshRevision.current += 1;
      if (sizeDebounceRef.current != null) clearTimeout(sizeDebounceRef.current);
      window.clearInterval(androidId);
      window.removeEventListener('focus', onFocus);
    };
  }, []);

  useEffect(() => {
    if (androidPrefs?.androidOverlayTrigger !== 'keyboard' || migratedLegacyTrigger.current) return;
    migratedLegacyTrigger.current = true;
    void updatePrefs((current) =>
      current.androidOverlayTrigger === 'keyboard'
        ? { ...current, androidOverlayTrigger: normalizeAndroidOverlayTrigger('keyboard') }
        : current,
    ).catch(() => undefined);
  }, [androidPrefs?.androidOverlayTrigger, updatePrefs]);

  const finishSizeDraft = (revision: number) => {
    if (draftRevision.current !== revision) return;
    draftSizeRef.current = null;
    if (loaded.current) setSizeDraft(null);
  };

  const saveAndroidOverlaySize = async (value: number, revision: number) => {
    setActionError(false);
    await updatePrefs((current) => ({ ...current, androidOverlaySizeDp: value })).catch(
      () => undefined,
    );
    finishSizeDraft(revision);
  };

  const handleAndroidOverlaySizeChange = (value: number) => {
    const clamped = clampAndroidOverlaySize(value);
    const revision = ++draftRevision.current;
    draftSizeRef.current = clamped;
    setSizeDraft(clamped);
    if (sizeDebounceRef.current != null) clearTimeout(sizeDebounceRef.current);
    sizeDebounceRef.current = window.setTimeout(() => {
      sizeDebounceRef.current = null;
      void saveAndroidOverlaySize(clamped, revision);
    }, 200);
  };

  const flushAndroidOverlaySizeSave = () => {
    if (sizeDebounceRef.current == null || draftSizeRef.current === null) return;
    clearTimeout(sizeDebounceRef.current);
    sizeDebounceRef.current = null;
    void saveAndroidOverlaySize(draftSizeRef.current, draftRevision.current);
  };

  const updateAndroidPref = async <K extends AndroidPreferenceKey>(
    key: K,
    value: UserPreferences[K] | ((current: UserPreferences) => UserPreferences[K]),
  ) => {
    const revision = draftRevision.current;
    const oldDraft = sizeDebounceRef.current == null ? null : draftSizeRef.current;
    if (sizeDebounceRef.current != null) {
      clearTimeout(sizeDebounceRef.current);
      sizeDebounceRef.current = null;
    }
    setActionError(false);
    try {
      // Compute the edit from the latest optimistic preferences in the shared
      // gate; fields saved by the onboarding or settings shell stay intact.
      await updatePrefs((current) => {
        const next = typeof value === 'function' ? value(current) : value;
        return {
          ...current,
          [key]:
            key === 'androidOverlayTrigger'
              ? normalizeAndroidOverlayTrigger(next as AndroidOverlayTrigger)
              : next,
          ...(oldDraft === null ? {} : { androidOverlaySizeDp: oldDraft }),
        };
      });
      await refreshAndroid();
    } catch {
      // The shared preferences owner rolls back and reports failed writes.
    } finally {
      if (oldDraft !== null) finishSizeDraft(revision);
    }
  };

  const showOverlayPermission =
    mode === 'all' || mode === 'permissionsOnly' || mode === 'overlayPermission';
  const showAccessibility =
    mode === 'all' || mode === 'permissionsOnly' || mode === 'accessibility';
  const showOverlayConfig = mode === 'all' || mode === 'overlayConfig';

  const rowJustify = layoutStack ? 'flex-start' : 'flex-end';
  const colAlign = layoutStack ? 'stretch' : 'flex-end';
  const hintAlign = layoutStack ? 'left' : 'right';
  const hintMaxWidth = layoutStack ? undefined : 300;
  const hintMaxWidthSm = layoutStack ? undefined : 220;
  const selectStyle = layoutStack
    ? { width: '100%', maxWidth: '100%' as const }
    : { minWidth: 180, maxWidth: '100%' as const };
  const actionRowStyle: CSSProperties = {
    display: 'flex',
    flexDirection: conservative ? 'column' : 'row',
    gap: 8,
    alignItems: conservative ? 'flex-start' : layoutStack ? 'flex-start' : 'center',
    justifyContent: rowJustify,
    width: '100%',
    flexWrap: conservative ? 'nowrap' : 'wrap',
    minWidth: 0,
  };

  return (
    <>
      {actionError && <p role="alert">{t('common.operationFailed')}</p>}
      {showOverlayPermission && (
        <SettingRow label={t('settings.permissions.androidOverlayLabel')}>
          <div className="ol-flex-row" style={actionRowStyle}>
            {androidOverlay?.message && (
              <span
                style={{
                  fontSize: 11.5,
                  color: 'var(--ol-ink-4)',
                  maxWidth: hintMaxWidthSm,
                  textAlign: hintAlign,
                }}
              >
                {androidOverlay.message}
              </span>
            )}
            <AndroidOverlayStatusPill status={androidOverlay} />
            {androidOverlay?.permission !== 'granted' && (
              <Btn
                variant="ghost"
                size="sm"
                onClick={() => {
                  void requestAndRefresh(requestAndroidOverlayPermission);
                }}
              >
                {t('settings.permissions.grant')}
              </Btn>
            )}
          </div>
        </SettingRow>
      )}
      {showAccessibility && (
        <SettingRow label={t('settings.permissions.androidAccessibilityLabel')}>
          <div
            style={{
              display: 'flex',
              flexDirection: 'column',
              gap: 6,
              alignItems: colAlign,
              width: '100%',
              minWidth: 0,
            }}
          >
            <div className="ol-flex-row" style={actionRowStyle}>
              {resolveAccessibilityMessage(t, androidAccessibility?.messageKey) && (
                <span
                  style={{
                    fontSize: 11.5,
                    color: 'var(--ol-ink-4)',
                    maxWidth: hintMaxWidthSm,
                    textAlign: hintAlign,
                  }}
                >
                  {resolveAccessibilityMessage(t, androidAccessibility?.messageKey)}
                </span>
              )}
              <AndroidAccessibilityStatusPill status={androidAccessibility} />
              {(!androidAccessibility?.enabled || androidAccessibility?.operational === false) && (
                <Btn
                  variant="ghost"
                  size="sm"
                  onClick={() => {
                    void requestAndRefresh(requestAndroidAccessibilityPermission);
                  }}
                >
                  {t('settings.permissions.openSystem')}
                </Btn>
              )}
            </div>
            <span
              style={{
                fontSize: 11,
                color: 'var(--ol-ink-4)',
                maxWidth: hintMaxWidth,
                textAlign: hintAlign,
              }}
            >
              {t('settings.permissions.androidAccessibilityImpact')}
            </span>
          </div>
        </SettingRow>
      )}
      {showAccessibility && (
        <SettingRow label={t('settings.permissions.androidShizukuLabel')}>
          <div
            style={{
              display: 'flex',
              flexDirection: 'column',
              gap: 6,
              alignItems: colAlign,
              width: '100%',
              minWidth: 0,
            }}
          >
            <div className="ol-flex-row" style={actionRowStyle}>
              {resolveShizukuMessage(t, shizukuDisplayMessageKey) && (
                <span
                  style={{
                    fontSize: 11.5,
                    color: 'var(--ol-ink-4)',
                    maxWidth: hintMaxWidthSm,
                    textAlign: hintAlign,
                  }}
                >
                  {resolveShizukuMessage(t, shizukuDisplayMessageKey)}
                </span>
              )}
              <AndroidShizukuStatusPill status={androidShizuku} />
              {(androidShizuku?.state === 'notInstalled' ||
                androidShizuku?.state === 'notRunning' ||
                androidShizuku?.state === 'binderDead') && (
                <Btn
                  variant="ghost"
                  size="sm"
                  onClick={() => {
                    setShizukuRecoveryMessageKey(null);
                    setShizukuActionMessageKey(null);
                    void requestAndRefresh(async () => {
                      const result = await openShizukuApp();
                      if (!result.launched && loaded.current) {
                        setShizukuActionMessageKey(result.messageKey);
                      }
                    });
                  }}
                >
                  {t('settings.permissions.androidShizukuOpenApp')}
                </Btn>
              )}
              {androidShizuku?.state === 'notAuthorized' && (
                <Btn
                  variant="ghost"
                  size="sm"
                  onClick={() => {
                    setShizukuRecoveryMessageKey(null);
                    setShizukuActionMessageKey(null);
                    void requestAndRefresh(async () => {
                      const result = await requestAndroidShizukuPermission();
                      if (!result.launched && loaded.current) {
                        setShizukuActionMessageKey(result.messageKey);
                      }
                    });
                  }}
                >
                  {t('settings.permissions.androidShizukuRequestPermission')}
                </Btn>
              )}
              {androidShizuku?.state === 'authorized' &&
                !androidShizuku.accessibility.operational && (
                  <Btn
                    variant="ghost"
                    size="sm"
                    disabled={shizukuRecoveryPending}
                    onClick={() => {
                      void handleRecoverAccessibility();
                    }}
                  >
                    {shizukuRecoveryPending
                      ? t('settings.permissions.checking')
                      : t('settings.permissions.androidShizukuRecover')}
                  </Btn>
                )}
              {shizukuRecoveryMessageKey &&
                androidShizuku?.state === 'authorized' &&
                !androidShizuku.accessibility.operational &&
                (shizukuRecoveryMessageKey === 'partial_rollback' ||
                  shizukuRecoveryMessageKey === 'manual_required' ||
                  shizukuRecoveryMessageKey === 'oem_rollback' ||
                  shizukuRecoveryMessageKey === 'concurrent_change') && (
                  <Btn
                    variant="ghost"
                    size="sm"
                    onClick={() => {
                      void requestAndRefresh(requestAndroidAccessibilityPermission);
                    }}
                  >
                    {t('settings.permissions.openSystem')}
                  </Btn>
                )}
            </div>
            {androidShizuku?.state === 'authorized' && (
              <span
                style={{
                  fontSize: 11,
                  color: 'var(--ol-ink-4)',
                  maxWidth: hintMaxWidth,
                  textAlign: hintAlign,
                }}
              >
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
            <span
              style={{
                fontSize: 11,
                color: 'var(--ol-ink-4)',
                maxWidth: hintMaxWidth,
                textAlign: hintAlign,
              }}
            >
              {t('settings.permissions.androidShizukuHint')}
            </span>
          </div>
        </SettingRow>
      )}
      {showOverlayConfig && (
        <>
          <SettingRow label={t('settings.permissions.androidInsertStrategyLabel')}>
            <div
              style={{
                display: 'flex',
                flexDirection: 'column',
                gap: 6,
                alignItems: colAlign,
                width: '100%',
              }}
            >
              <select
                disabled={!androidPrefs}
                value={androidPrefs?.androidInsertStrategy ?? 'accessibility'}
                onChange={(event) => {
                  void updateAndroidPref(
                    'androidInsertStrategy',
                    event.target.value as AndroidInsertStrategy,
                  );
                }}
                style={selectStyle}
              >
                <option value="accessibility">
                  {t('settings.permissions.androidInsertStrategy.accessibility')}
                </option>
                <option value="clipboard">
                  {t('settings.permissions.androidInsertStrategy.clipboard')}
                </option>
              </select>
              <span style={{ fontSize: 11, color: 'var(--ol-ink-4)', textAlign: hintAlign }}>
                {t(
                  `settings.permissions.androidInsertStrategyHint.${androidPrefs?.androidInsertStrategy ?? 'accessibility'}`,
                )}
              </span>
            </div>
          </SettingRow>
          <SettingRow label={t('settings.permissions.androidOverlayTriggerLabel')}>
            <div
              style={{
                display: 'flex',
                flexDirection: 'column',
                gap: 6,
                alignItems: colAlign,
                width: '100%',
              }}
            >
              <select
                disabled={!androidPrefs}
                value={androidPrefs?.androidOverlayTrigger ?? 'background'}
                onChange={(event) => {
                  void updateAndroidPref(
                    'androidOverlayTrigger',
                    event.target.value as AndroidOverlayTrigger,
                  );
                }}
                style={selectStyle}
              >
                <option value="background">
                  {t('settings.permissions.androidOverlayTrigger.background')}
                </option>
                <option value="keyboard" disabled>
                  {t('settings.permissions.androidOverlayTrigger.keyboard')}
                </option>
                <option value="always">
                  {t('settings.permissions.androidOverlayTrigger.always')}
                </option>
                <option value="never">
                  {t('settings.permissions.androidOverlayTrigger.never')}
                </option>
              </select>
              <span style={{ fontSize: 11, color: 'var(--ol-ink-4)', textAlign: hintAlign }}>
                {t(
                  `settings.permissions.androidOverlayTriggerHint.${androidPrefs?.androidOverlayTrigger ?? 'background'}`,
                )}
              </span>
              <span style={{ fontSize: 11, color: 'var(--ol-ink-4)', textAlign: hintAlign }}>
                {t('settings.permissions.androidOverlayTriggerDisabled.keyboard')}
              </span>
            </div>
          </SettingRow>
          <SettingRow label={t('settings.permissions.androidOverlayActivationModeLabel')}>
            <div
              style={{
                display: 'flex',
                flexDirection: 'column',
                gap: 6,
                alignItems: colAlign,
                width: '100%',
              }}
            >
              <select
                disabled={!androidPrefs}
                value={androidPrefs?.androidOverlayActivationMode ?? 'tap'}
                onChange={(event) => {
                  void updateAndroidPref(
                    'androidOverlayActivationMode',
                    event.target.value as AndroidOverlayActivationMode,
                  );
                }}
                style={selectStyle}
              >
                <option value="tap">
                  {t('settings.permissions.androidOverlayActivationMode.tap')}
                </option>
                <option value="long_press">
                  {t('settings.permissions.androidOverlayActivationMode.long_press')}
                </option>
              </select>
              <span style={{ fontSize: 11, color: 'var(--ol-ink-4)', textAlign: hintAlign }}>
                {t(
                  `settings.permissions.androidOverlayActivationModeHint.${androidPrefs?.androidOverlayActivationMode ?? 'tap'}`,
                )}
              </span>
            </div>
          </SettingRow>
          <SettingRow
            label={t(
              'settings.permissions.androidOverlayGestureActionsLabel',
              'Overlay swipe actions',
            )}
            desc={t(
              'settings.permissions.androidOverlayGestureActionsDesc',
              'These actions apply while recording. A normal tap finishes ordinary dictation; a Quick note swipe keeps the audio permanently.',
            )}
          >
            <div
              style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(2, minmax(0, 1fr))',
                gap: 8,
                width: '100%',
              }}
            >
              {(['up', 'down', 'left', 'right'] as const).map((direction) => (
                <label
                  key={direction}
                  style={{ display: 'flex', flexDirection: 'column', gap: 4, minWidth: 0 }}
                >
                  <span style={{ fontSize: 11, color: 'var(--ol-ink-3)' }}>
                    {t(
                      `settings.permissions.androidOverlayGestureDirection.${direction}`,
                      direction,
                    )}
                  </span>
                  <select
                    disabled={!androidPrefs}
                    value={androidPrefs?.androidOverlayGestureActions?.[direction] ?? 'none'}
                    onChange={(event) => {
                      const action = event.currentTarget.value as AndroidOverlayGestureAction;
                      void updateAndroidPref('androidOverlayGestureActions', (current) => ({
                        ...(current.androidOverlayGestureActions ?? {
                          up: 'cancel',
                          down: 'none',
                          left: 'translation',
                          right: 'qa',
                        }),
                        [direction]: action,
                      }));
                    }}
                    style={selectStyle}
                  >
                    {(
                      [
                        'none',
                        'quick_note',
                        'translation',
                        'style_pack',
                        'cancel',
                        'qa',
                        'voice_edit',
                      ] as const
                    ).map((action) => (
                      <option key={action} value={action}>
                        {t(`settings.permissions.androidOverlayGestureAction.${action}`, action)}
                      </option>
                    ))}
                  </select>
                </label>
              ))}
            </div>
          </SettingRow>
          <SettingRow label={t('settings.permissions.androidOverlaySizeLabel')}>
            <div
              style={{
                display: 'flex',
                flexDirection: 'column',
                gap: 6,
                alignItems: colAlign,
                width: '100%',
              }}
            >
              <div
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: 10,
                  width: '100%',
                  maxWidth: '100%',
                }}
              >
                <SolidSlider
                  aria-label={t('settings.permissions.androidOverlaySizeLabel')}
                  disabled={!androidPrefs}
                  min={48}
                  max={120}
                  step={4}
                  value={sizeDraft ?? androidPrefs?.androidOverlaySizeDp ?? 72}
                  onValueChange={handleAndroidOverlaySizeChange}
                  onValueCommitted={flushAndroidOverlaySizeSave}
                  style={{ flex: 1, minWidth: 0 }}
                />
                <span
                  style={{
                    fontSize: 12,
                    color: 'var(--ol-ink-3)',
                    minWidth: 42,
                    textAlign: hintAlign,
                  }}
                >
                  {sizeDraft ?? androidPrefs?.androidOverlaySizeDp ?? 72} dp
                </span>
              </div>
              <span style={{ fontSize: 11, color: 'var(--ol-ink-4)', textAlign: hintAlign }}>
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
    return (
      <Pill tone="ok">
        <Icon name="check" size={11} />
        {t('settings.permissions.granted')}
      </Pill>
    );
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
    return (
      <Pill tone="ok">
        <Icon name="check" size={11} />
        {t('settings.permissions.granted')}
      </Pill>
    );
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
    return (
      <Pill tone="ok">
        <Icon name="check" size={11} />
        {t(labelKey)}
      </Pill>
    );
  }
  if (status.state === 'notAndroid') {
    return <Pill tone="default">{t(labelKey)}</Pill>;
  }
  return <Pill tone="outline">{t(labelKey)}</Pill>;
}
