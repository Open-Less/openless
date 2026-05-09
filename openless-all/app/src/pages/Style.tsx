// Style.tsx — 接 getSettings / setDefaultPolishMode / setStyleEnabled。
// defaultMode 来自 prefs.defaultMode，启停从 prefs.enabledModes 反推。

import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { getSettings, setDefaultPolishMode, setStyleEnabled, setSettings } from '../lib/ipc';
import type { PolishMode, UserPreferences } from '../lib/types';
import {
  applyStylePreferencesNotification,
  persistStylePreferenceChange,
  rollbackDefaultModeChange,
  rollbackStyleEnabledChange,
  rollbackWholeStylePreferences,
} from '../lib/stylePrefs';
import { PageHeader, Pill } from './_atoms';
import { useHotkeySettings } from '../state/HotkeySettingsContext';

interface StyleDef {
  id: PolishMode;
  name: string;
  desc: string;
  sample: string;
}

const STYLE_IDS: PolishMode[] = ['raw', 'light', 'structured', 'formal'];
type StyleSaveErrorTarget = PolishMode | 'master';

export function Style() {
  const { t } = useTranslation();
  const { prefs: sharedPrefs } = useHotkeySettings();
  const STYLES: StyleDef[] = STYLE_IDS.map(id => ({
    id,
    name: t(`style.modes.${id}.name`),
    desc: t(`style.modes.${id}.desc`),
    sample: t(`style.modes.${id}.sample`),
  }));
  const [prefs, setPrefs] = useState<UserPreferences | null>(null);
  const [saveError, setSaveError] = useState<{ target: StyleSaveErrorTarget; message: string } | null>(null);

  useEffect(() => {
    getSettings().then(setPrefs);
  }, []);

  useEffect(() => {
    if (!sharedPrefs) return;
    setPrefs(current => applyStylePreferencesNotification(current, sharedPrefs));
    setSaveError(null);
  }, [sharedPrefs]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    (async () => {
      try {
        const { listen } = await import('@tauri-apps/api/event');
        unlisten = await listen<UserPreferences>('prefs:changed', event => {
          setPrefs(current => applyStylePreferencesNotification(current, event.payload));
          setSaveError(null);
        });
        if (cancelled && unlisten) unlisten();
      } catch (error) {
        console.warn('[style] prefs:changed listener setup failed', error);
      }
    })();
    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, []);

  const showSaveError = (target: StyleSaveErrorTarget, error: string) => {
    setSaveError({ target, message: t('style.saveFailed', { error }) });
  };

  const onPickDefault = async (mode: PolishMode) => {
    if (!prefs) return;
    const next = { ...prefs, defaultMode: mode };
    const saved = await persistStylePreferenceChange(
      next,
      () => setDefaultPolishMode(mode),
      setPrefs,
      error => showSaveError(mode, error),
      rollbackDefaultModeChange(prefs, next),
    );
    if (saved) setSaveError(null);
  };

  const onToggleEnabled = async (mode: PolishMode) => {
    if (!prefs) return;
    const enabled = !prefs.enabledModes.includes(mode);
    const nextEnabled = enabled
      ? [...prefs.enabledModes, mode]
      : prefs.enabledModes.filter(m => m !== mode);
    const next = { ...prefs, enabledModes: nextEnabled };
    const saved = await persistStylePreferenceChange(
      next,
      () => setStyleEnabled(mode, enabled),
      setPrefs,
      error => showSaveError(mode, error),
      rollbackStyleEnabledChange(mode, prefs, next),
    );
    if (saved) setSaveError(null);
  };

  // Universal directives: persist to backend on blur (avoid 1 IPC per keystroke).
  // Local state mirrors `prefs.polishUniversalDirectives` so typing feels instant;
  // we commit on blur via setSettings() and rely on prefs:changed to re-sync.
  const [universalDraft, setUniversalDraft] = useState<string>('');
  useEffect(() => {
    if (prefs) setUniversalDraft(prefs.polishUniversalDirectives ?? '');
  }, [prefs?.polishUniversalDirectives]);
  const onCommitUniversalDirectives = async () => {
    if (!prefs) return;
    const trimmed = universalDraft;
    if ((prefs.polishUniversalDirectives ?? '') === trimmed) return;
    const next = { ...prefs, polishUniversalDirectives: trimmed };
    const saved = await persistStylePreferenceChange(
      next,
      () => setSettings(next),
      setPrefs,
      error => showSaveError('master', error),
      rollbackWholeStylePreferences(prefs, next),
    );
    if (saved) setSaveError(null);
  };

  if (!prefs) {
    return (
      <PageHeader
        kicker={t('style.kicker')}
        title={t('style.title')}
        desc={t('common.loading')}
      />
    );
  }

  const masterEnabled = prefs.enabledModes.length > 0;

  const onMasterToggle = async () => {
    if (!prefs) return;
    if (masterEnabled) {
      // 全部关闭 → 留 raw 和当前 default 兜底
      const next = { ...prefs, enabledModes: [] as PolishMode[] };
      const saved = await persistStylePreferenceChange(
        next,
        () => setSettings(next),
        setPrefs,
        error => showSaveError('master', error),
        rollbackWholeStylePreferences(prefs, next),
      );
      if (saved) setSaveError(null);
    } else {
      const next = { ...prefs, enabledModes: ['raw', 'light', 'structured', 'formal'] as PolishMode[] };
      const saved = await persistStylePreferenceChange(
        next,
        () => setSettings(next),
        setPrefs,
        error => showSaveError('master', error),
        rollbackWholeStylePreferences(prefs, next),
      );
      if (saved) setSaveError(null);
    }
  };

  return (
    <>
      <PageHeader
        kicker={t('style.kicker')}
        title={t('style.title')}
        desc={t('style.desc')}
        right={
          <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
            <span style={{ fontSize: 12, color: 'var(--ol-ink-3)' }}>{t('style.masterToggle')}</span>
            <button
              onClick={onMasterToggle}
              style={{
                position: 'relative', width: 36, height: 20, borderRadius: 999, border: 0,
                background: masterEnabled ? 'var(--ol-blue)' : 'rgba(0,0,0,0.15)',
                cursor: 'default', transition: 'background 0.16s var(--ol-motion-quick)',
              }}
            >
              <span
                style={{
                  position: 'absolute', top: 2, left: masterEnabled ? 18 : 2,
                  width: 16, height: 16, borderRadius: 999, background: '#fff',
                  boxShadow: '0 1px 2px rgba(0,0,0,.2)', transition: 'left .16s var(--ol-motion-spring)',
                }}
              />
            </button>
            {saveError?.target === 'master' && (
              <span
                role="alert"
                style={{ fontSize: 11.5, color: 'var(--ol-red, #ef4444)', maxWidth: 220, lineHeight: 1.45 }}
              >
                {saveError.message}
              </span>
            )}
          </div>
        }
      />
      <div
        style={{
          padding: 16,
          marginBottom: 16,
          background: 'var(--ol-surface)',
          border: '0.5px solid var(--ol-line)',
          borderRadius: 'var(--ol-r-lg)',
          boxShadow: 'var(--ol-shadow-sm)',
        }}
      >
        <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--ol-ink)', marginBottom: 4 }}>
          {t('style.universalDirectivesLabel')}
        </div>
        <div style={{ fontSize: 11.5, color: 'var(--ol-ink-3)', marginBottom: 10, lineHeight: 1.5 }}>
          {t('style.universalDirectivesDesc')}
        </div>
        <textarea
          value={universalDraft}
          onChange={e => setUniversalDraft(e.target.value)}
          onBlur={onCommitUniversalDirectives}
          placeholder={t('style.universalDirectivesPlaceholder')}
          rows={4}
          style={{
            width: '100%',
            padding: 10,
            fontSize: 12.5,
            fontFamily: 'inherit',
            lineHeight: 1.55,
            color: 'var(--ol-ink)',
            background: 'var(--ol-surface-2)',
            border: '0.5px solid var(--ol-line)',
            borderRadius: 8,
            resize: 'vertical',
            boxSizing: 'border-box',
            outline: 'none',
          }}
        />
      </div>
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
        {STYLES.map(s => {
          const isDefault = prefs.defaultMode === s.id;
          const isEnabled = prefs.enabledModes.includes(s.id);
          return (
            <div
              key={s.id}
              style={{
                padding: 18,
                background: 'var(--ol-surface)',
                border: '0.5px solid ' + (isDefault ? 'var(--ol-blue)' : 'var(--ol-line)'),
                borderRadius: 'var(--ol-r-lg)',
                boxShadow: isDefault ? '0 0 0 3px var(--ol-blue-ring), var(--ol-shadow-sm)' : 'var(--ol-shadow-sm)',
                opacity: isEnabled ? 1 : 0.55,
                position: 'relative',
                transition: 'border-color 0.16s var(--ol-motion-quick), box-shadow 0.18s var(--ol-motion-soft), opacity 0.18s var(--ol-motion-soft)',
              }}
            >
              <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 4 }}>
                <button
                  onClick={() => onPickDefault(s.id)}
                  aria-label={t('style.ariaSetDefault')}
                  style={{
                    width: 16, height: 16, padding: 0, border: 0, borderRadius: 999,
                    background: isDefault ? 'var(--ol-blue)' : 'transparent',
                    boxShadow: isDefault ? 'none' : 'inset 0 0 0 1.5px var(--ol-line-strong)',
                    color: '#fff', cursor: 'default',
                    display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
                  }}
                >
                  {isDefault && (
                    <svg width="9" height="9" viewBox="0 0 9 9"><path d="M1.5 4.5l2.5 2.5 4-5" stroke="currentColor" strokeWidth="1.5" fill="none" strokeLinecap="round" strokeLinejoin="round" /></svg>
                  )}
                </button>
                <button
                  onClick={() => onPickDefault(s.id)}
                  style={{
                    background: 'transparent', border: 0, padding: 0,
                    fontSize: 14, fontWeight: 600, fontFamily: 'inherit',
                    color: 'var(--ol-ink)', cursor: 'default',
                  }}
                >
                  {s.name}
                </button>
                {isDefault && <Pill tone="blue" size="sm" style={{ marginLeft: 'auto' }}>{t('style.currentDefault')}</Pill>}
                {!isDefault && (
                  <button
                    onClick={() => onToggleEnabled(s.id)}
                    style={{
                      marginLeft: 'auto',
                      position: 'relative', width: 30, height: 18, borderRadius: 999, border: 0,
                      background: isEnabled ? 'var(--ol-blue)' : 'rgba(0,0,0,0.15)',
                      cursor: 'default',
                      transition: 'background 0.16s var(--ol-motion-quick)',
                    }}
                  >
                    <span style={{
                      position: 'absolute', top: 2, left: isEnabled ? 14 : 2,
                      width: 14, height: 14, borderRadius: 999, background: '#fff',
                      boxShadow: '0 1px 2px rgba(0,0,0,.2)', transition: 'left .16s var(--ol-motion-spring)',
                    }} />
                  </button>
                )}
              </div>
              <div style={{ fontSize: 11.5, color: 'var(--ol-ink-3)', marginBottom: 12 }}>{s.desc}</div>
              <div
                style={{
                  fontSize: 12.5, color: 'var(--ol-ink-2)', lineHeight: 1.6,
                  padding: 12, borderRadius: 8,
                  background: 'var(--ol-surface-2)',
                  border: '0.5px dashed var(--ol-line)',
                  whiteSpace: 'pre-line',
                }}
              >
                {s.sample}
              </div>
              {saveError?.target === s.id && (
                <div
                  role="alert"
                  style={{ marginTop: 10, fontSize: 11.5, color: 'var(--ol-red, #ef4444)', lineHeight: 1.45 }}
                >
                  {saveError.message}
                </div>
              )}
            </div>
          );
        })}
      </div>
    </>
  );
}
