// Style.tsx — ビルトイン4スタイル + ユーザー定義カスタムスタイル。
// defaultMode は prefs.defaultMode、有効/無効は prefs.enabledModes 反映。
// カスタムスタイルは prefs.customModes（順序保持）。`PolishMode` は `'custom:<id>'` 形式。

import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  getSettings,
  setDefaultPolishMode,
  setStyleEnabled,
  setSettings,
  getDefaultPolishPrompt,
  addCustomMode,
  updateCustomMode,
  deleteCustomMode,
  setAppModeOverrides,
} from '../lib/ipc';
import type { AppModeOverride, CustomMode, PolishMode, UserPreferences } from '../lib/types';
import {
  persistStylePreferenceChange,
  rollbackDefaultModeChange,
  rollbackStyleEnabledChange,
  rollbackWholeStylePreferences,
} from '../lib/stylePrefs';
import { PageHeader, Pill } from './_atoms';

interface StyleDef {
  id: PolishMode;
  name: string;
  desc: string;
  sample: string;
}

const BUILTIN_IDS: Array<'raw' | 'light' | 'structured' | 'formal'> = ['raw', 'light', 'structured', 'formal'];
type StyleSaveErrorTarget = PolishMode | 'master' | 'custom';

interface CustomFormState {
  // 編集対象 id（新規追加なら空文字列）。
  id: string;
  name: string;
  prompt: string;
  // true = 新規、false = 既存編集
  isNew: boolean;
  errorMessage: string;
}

export function Style() {
  const { t } = useTranslation();
  const STYLES: StyleDef[] = BUILTIN_IDS.map(id => ({
    id,
    name: t(`style.modes.${id}.name`),
    desc: t(`style.modes.${id}.desc`),
    sample: t(`style.modes.${id}.sample`),
  }));
  const [prefs, setPrefs] = useState<UserPreferences | null>(null);
  const [saveError, setSaveError] = useState<{ target: StyleSaveErrorTarget; message: string } | null>(null);
  // ビルトイン4 mode の既定 prompt キャッシュ。「prompt を見る」展開時に表示。
  const [defaultPrompts, setDefaultPrompts] = useState<Record<string, string>>({});
  // 各ビルトイン mode で prompt を展開しているか。
  const [expandedBuiltin, setExpandedBuiltin] = useState<Record<string, boolean>>({});
  // カスタム編集フォーム（null = フォーム閉じている）。
  const [customForm, setCustomForm] = useState<CustomFormState | null>(null);
  // アプリ別自動切替の編集中配列。null = まだ初期化されていない（prefs 取得待ち）。
  const [overrides, setOverrides] = useState<AppModeOverride[] | null>(null);
  const [overrideError, setOverrideError] = useState<string | null>(null);
  // 500ms debounce 用の timer。直近の編集を 500ms 何もせず待ったら setAppModeOverrides を呼ぶ。
  const overrideDebounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    getSettings().then(p => {
      setPrefs(p);
      setOverrides(p.appModeOverrides);
    });
  }, []);

  // アンマウント時に保留中の debounce をクリア。残ったままだと unmount 後に invoke が走る。
  useEffect(() => {
    return () => {
      if (overrideDebounceRef.current) {
        clearTimeout(overrideDebounceRef.current);
      }
    };
  }, []);

  const scheduleOverrideSave = (next: AppModeOverride[]) => {
    if (overrideDebounceRef.current) {
      clearTimeout(overrideDebounceRef.current);
    }
    overrideDebounceRef.current = setTimeout(async () => {
      try {
        await setAppModeOverrides(next);
        setOverrideError(null);
        // 後端が prefs:changed を emit しても今のページは listen していないので
        // ローカル prefs を即座に同期しておく（次の getSettings 呼び出しまで diverge しないため）。
        setPrefs(prev => (prev ? { ...prev, appModeOverrides: next } : prev));
      } catch (err) {
        setOverrideError(String(err));
      }
    }, 500);
  };

  const updateOverridesLocally = (next: AppModeOverride[]) => {
    setOverrides(next);
    scheduleOverrideSave(next);
  };

  const onAddOverride = () => {
    if (!overrides) return;
    updateOverridesLocally([...overrides, { appPattern: '', mode: 'light' }]);
  };

  const onChangeOverridePattern = (idx: number, pattern: string) => {
    if (!overrides) return;
    const next = overrides.slice();
    next[idx] = { ...next[idx], appPattern: pattern };
    updateOverridesLocally(next);
  };

  const onChangeOverrideMode = (idx: number, mode: PolishMode) => {
    if (!overrides) return;
    const next = overrides.slice();
    next[idx] = { ...next[idx], mode };
    updateOverridesLocally(next);
  };

  const onDeleteOverride = (idx: number) => {
    if (!overrides) return;
    const next = overrides.filter((_, i) => i !== idx);
    updateOverridesLocally(next);
  };

  const showSaveError = (target: StyleSaveErrorTarget, error: string) => {
    setSaveError({ target, message: t('style.saveFailed', { error }) });
  };

  const ensureBuiltinPromptLoaded = async (id: string) => {
    if (defaultPrompts[id] !== undefined) return;
    try {
      const text = await getDefaultPolishPrompt(id);
      setDefaultPrompts(prev => ({ ...prev, [id]: text }));
    } catch {
      // 失敗時は空のまま — トグル状態に「読み込めません」と書く方がうるさい。
    }
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

  const openCustomFormForNew = () => {
    setCustomForm({ id: '', name: '', prompt: '', isNew: true, errorMessage: '' });
  };

  const openCustomFormForEdit = (cm: CustomMode) => {
    setCustomForm({ id: cm.id, name: cm.name, prompt: cm.prompt, isNew: false, errorMessage: '' });
  };

  const closeCustomForm = () => setCustomForm(null);

  const onCustomFormSave = async () => {
    if (!customForm || !prefs) return;
    const trimmedId = customForm.id.trim();
    const trimmedName = customForm.name.trim();
    const trimmedPrompt = customForm.prompt.trim();
    if (customForm.isNew) {
      if (!trimmedId) {
        setCustomForm({ ...customForm, errorMessage: t('style.customMode.idEmpty') });
        return;
      }
      if (!/^[A-Za-z0-9-]+$/.test(trimmedId)) {
        setCustomForm({ ...customForm, errorMessage: t('style.customMode.idInvalid') });
        return;
      }
    }
    if (!trimmedName) {
      setCustomForm({ ...customForm, errorMessage: t('style.customMode.nameEmpty') });
      return;
    }
    if (!trimmedPrompt) {
      setCustomForm({ ...customForm, errorMessage: t('style.customMode.promptEmpty') });
      return;
    }
    try {
      if (customForm.isNew) {
        await addCustomMode(trimmedId, trimmedName, customForm.prompt);
      } else {
        await updateCustomMode(customForm.id, trimmedName, customForm.prompt);
      }
      // 保存成功 → 設定再取得して反映
      const fresh = await getSettings();
      setPrefs(fresh);
      setCustomForm(null);
      setSaveError(null);
    } catch (error) {
      setCustomForm({ ...customForm, errorMessage: String(error) });
    }
  };

  const onCustomDelete = async (cm: CustomMode) => {
    if (!prefs) return;
    if (!confirm(t('style.customMode.confirmDelete'))) return;
    try {
      await deleteCustomMode(cm.id);
      const fresh = await getSettings();
      setPrefs(fresh);
      setSaveError(null);
    } catch (error) {
      showSaveError('custom', String(error));
    }
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
      const next = {
        ...prefs,
        enabledModes: [
          ...(['raw', 'light', 'structured', 'formal'] as PolishMode[]),
          ...prefs.customModes.map(cm => `custom:${cm.id}` as PolishMode),
        ],
      };
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

  const renderModeCard = (
    modeId: PolishMode,
    name: string,
    desc: string | null,
    sampleSlot: React.ReactNode,
    extraSlot?: React.ReactNode,
  ) => {
    const isDefault = prefs.defaultMode === modeId;
    const isEnabled = prefs.enabledModes.includes(modeId);
    return (
      <div
        key={String(modeId)}
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
            onClick={() => onPickDefault(modeId)}
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
            onClick={() => onPickDefault(modeId)}
            style={{
              background: 'transparent', border: 0, padding: 0,
              fontSize: 14, fontWeight: 600, fontFamily: 'inherit',
              color: 'var(--ol-ink)', cursor: 'default',
            }}
          >
            {name}
          </button>
          {isDefault && <Pill tone="blue" size="sm" style={{ marginLeft: 'auto' }}>{t('style.currentDefault')}</Pill>}
          {!isDefault && (
            <button
              onClick={() => onToggleEnabled(modeId)}
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
        {desc && <div style={{ fontSize: 11.5, color: 'var(--ol-ink-3)', marginBottom: 12 }}>{desc}</div>}
        {sampleSlot}
        {extraSlot}
        {saveError?.target === modeId && (
          <div
            role="alert"
            style={{ marginTop: 10, fontSize: 11.5, color: 'var(--ol-red, #ef4444)', lineHeight: 1.45 }}
          >
            {saveError.message}
          </div>
        )}
      </div>
    );
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
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
        {STYLES.map(s => {
          const expanded = expandedBuiltin[s.id] === true;
          const promptText = defaultPrompts[s.id];
          const sampleSlot = (
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
          );
          const extraSlot = (
            <div style={{ marginTop: 10 }}>
              <button
                onClick={async () => {
                  if (!expanded) await ensureBuiltinPromptLoaded(s.id);
                  setExpandedBuiltin(prev => ({ ...prev, [s.id]: !prev[s.id] }));
                }}
                style={{
                  background: 'transparent',
                  border: 0,
                  padding: 0,
                  fontSize: 11.5,
                  color: 'var(--ol-blue)',
                  cursor: 'default',
                  fontFamily: 'inherit',
                }}
              >
                {expanded ? t('style.customMode.hideBuiltinPrompt') : t('style.customMode.viewBuiltinPrompt')}
              </button>
              {expanded && (
                <pre
                  style={{
                    marginTop: 8,
                    padding: 10,
                    fontSize: 11,
                    lineHeight: 1.55,
                    fontFamily: 'inherit',
                    color: 'var(--ol-ink-2)',
                    background: 'var(--ol-surface-2)',
                    border: '0.5px solid var(--ol-line)',
                    borderRadius: 'var(--ol-r-md)',
                    whiteSpace: 'pre-wrap',
                    maxHeight: 240,
                    overflowY: 'auto',
                  }}
                >
                  {promptText ?? '…'}
                </pre>
              )}
            </div>
          );
          return renderModeCard(s.id, s.name, s.desc, sampleSlot, extraSlot);
        })}
      </div>

      {/* カスタムスタイル領域 */}
      <div
        style={{
          marginTop: 24,
          padding: 18,
          background: 'var(--ol-surface)',
          border: '0.5px solid var(--ol-line)',
          borderRadius: 'var(--ol-r-lg)',
          boxShadow: 'var(--ol-shadow-sm)',
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginBottom: 4 }}>
          <div style={{ fontSize: 14, fontWeight: 600, color: 'var(--ol-ink)' }}>
            {t('style.customMode.title')}
          </div>
          <button
            onClick={openCustomFormForNew}
            style={{
              marginLeft: 'auto',
              padding: '4px 10px',
              fontSize: 12,
              borderRadius: 'var(--ol-r-sm)',
              border: '0.5px solid var(--ol-line-strong)',
              background: 'var(--ol-surface-2)',
              color: 'var(--ol-ink)',
              cursor: 'default',
              fontFamily: 'inherit',
            }}
          >
            {t('style.customMode.addButton')}
          </button>
        </div>
        <div style={{ fontSize: 11.5, color: 'var(--ol-ink-3)', marginBottom: 14, lineHeight: 1.5 }}>
          {t('style.customMode.desc')}
        </div>

        {customForm && (
          <div
            style={{
              padding: 14,
              marginBottom: 14,
              background: 'var(--ol-surface-2)',
              border: '0.5px solid var(--ol-line-strong)',
              borderRadius: 'var(--ol-r-md)',
              display: 'flex',
              flexDirection: 'column',
              gap: 10,
            }}
          >
            <label style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
              <span style={{ fontSize: 11.5, color: 'var(--ol-ink-3)' }}>{t('style.customMode.idLabel')}</span>
              <input
                value={customForm.id}
                onChange={e => setCustomForm({ ...customForm, id: e.target.value })}
                placeholder={t('style.customMode.idPlaceholder')}
                disabled={!customForm.isNew}
                style={{
                  padding: '6px 8px',
                  fontSize: 12,
                  fontFamily: 'inherit',
                  border: '0.5px solid var(--ol-line-strong)',
                  borderRadius: 'var(--ol-r-sm)',
                  background: customForm.isNew ? 'var(--ol-surface)' : 'var(--ol-surface-2)',
                  color: customForm.isNew ? 'var(--ol-ink)' : 'var(--ol-ink-3)',
                }}
              />
              <span style={{ fontSize: 10.5, color: 'var(--ol-ink-3)' }}>{t('style.customMode.idHint')}</span>
            </label>
            <label style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
              <span style={{ fontSize: 11.5, color: 'var(--ol-ink-3)' }}>{t('style.customMode.nameLabel')}</span>
              <input
                value={customForm.name}
                onChange={e => setCustomForm({ ...customForm, name: e.target.value })}
                placeholder={t('style.customMode.namePlaceholder')}
                style={{
                  padding: '6px 8px',
                  fontSize: 12,
                  border: '0.5px solid var(--ol-line-strong)',
                  borderRadius: 'var(--ol-r-sm)',
                  background: 'var(--ol-surface)',
                  color: 'var(--ol-ink)',
                  fontFamily: 'inherit',
                }}
              />
            </label>
            <label style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
              <span style={{ fontSize: 11.5, color: 'var(--ol-ink-3)' }}>{t('style.customMode.promptLabel')}</span>
              <textarea
                value={customForm.prompt}
                onChange={e => setCustomForm({ ...customForm, prompt: e.target.value })}
                placeholder={t('style.customMode.promptPlaceholder')}
                rows={10}
                spellCheck={false}
                style={{
                  width: '100%',
                  boxSizing: 'border-box',
                  padding: '8px 10px',
                  fontSize: 12,
                  lineHeight: 1.55,
                  fontFamily: 'inherit',
                  color: 'var(--ol-ink)',
                  background: 'var(--ol-surface)',
                  border: '0.5px solid var(--ol-line-strong)',
                  borderRadius: 'var(--ol-r-md)',
                  resize: 'vertical',
                }}
              />
            </label>
            {customForm.errorMessage && (
              <div role="alert" style={{ fontSize: 11, color: 'var(--ol-red, #ef4444)' }}>
                {customForm.errorMessage}
              </div>
            )}
            <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
              <button
                onClick={closeCustomForm}
                style={{
                  padding: '4px 12px',
                  fontSize: 12,
                  borderRadius: 'var(--ol-r-sm)',
                  border: '0.5px solid var(--ol-line-strong)',
                  background: 'var(--ol-surface)',
                  color: 'var(--ol-ink)',
                  cursor: 'default',
                  fontFamily: 'inherit',
                }}
              >
                {t('style.customMode.cancel')}
              </button>
              <button
                onClick={onCustomFormSave}
                style={{
                  padding: '4px 12px',
                  fontSize: 12,
                  borderRadius: 'var(--ol-r-sm)',
                  border: 0,
                  background: 'var(--ol-blue)',
                  color: '#fff',
                  cursor: 'default',
                  fontFamily: 'inherit',
                }}
              >
                {t('style.customMode.save')}
              </button>
            </div>
          </div>
        )}

        {prefs.customModes.length === 0 && !customForm && (
          <div style={{ fontSize: 12, color: 'var(--ol-ink-3)', padding: 12 }}>
            {t('style.customMode.empty')}
          </div>
        )}

        {prefs.customModes.length > 0 && (
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
            {prefs.customModes.map(cm => {
              const modeId: PolishMode = `custom:${cm.id}`;
              const sampleSlot = (
                <div
                  style={{
                    fontSize: 12, color: 'var(--ol-ink-3)', lineHeight: 1.55,
                    padding: 10, borderRadius: 8,
                    background: 'var(--ol-surface-2)',
                    border: '0.5px dashed var(--ol-line)',
                    whiteSpace: 'pre-wrap',
                    fontFamily: 'inherit',
                    maxHeight: 140,
                    overflowY: 'auto',
                  }}
                >
                  {cm.prompt}
                </div>
              );
              const extraSlot = (
                <div style={{ marginTop: 10, display: 'flex', gap: 8 }}>
                  <button
                    onClick={() => openCustomFormForEdit(cm)}
                    style={{
                      padding: '3px 10px',
                      fontSize: 11,
                      borderRadius: 'var(--ol-r-sm)',
                      border: '0.5px solid var(--ol-line-strong)',
                      background: 'var(--ol-surface)',
                      color: 'var(--ol-ink)',
                      cursor: 'default',
                      fontFamily: 'inherit',
                    }}
                  >
                    {t('style.customMode.edit')}
                  </button>
                  <button
                    onClick={() => onCustomDelete(cm)}
                    style={{
                      padding: '3px 10px',
                      fontSize: 11,
                      borderRadius: 'var(--ol-r-sm)',
                      border: '0.5px solid var(--ol-line-strong)',
                      background: 'var(--ol-surface)',
                      color: 'var(--ol-red, #ef4444)',
                      cursor: 'default',
                      fontFamily: 'inherit',
                    }}
                  >
                    {t('style.customMode.delete')}
                  </button>
                </div>
              );
              return renderModeCard(modeId, cm.name, null, sampleSlot, extraSlot);
            })}
          </div>
        )}

        {saveError?.target === 'custom' && (
          <div
            role="alert"
            style={{ marginTop: 10, fontSize: 11.5, color: 'var(--ol-red, #ef4444)', lineHeight: 1.45 }}
          >
            {saveError.message}
          </div>
        )}
      </div>

      {/* アプリ別自動切替セクション */}
      <div
        style={{
          marginTop: 24,
          padding: 18,
          background: 'var(--ol-surface)',
          border: '0.5px solid var(--ol-line)',
          borderRadius: 'var(--ol-r-lg)',
          boxShadow: 'var(--ol-shadow-sm)',
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginBottom: 4 }}>
          <div style={{ fontSize: 14, fontWeight: 600, color: 'var(--ol-ink)' }}>
            {t('style.appOverride.title')}
          </div>
          <button
            onClick={onAddOverride}
            style={{
              marginLeft: 'auto',
              padding: '4px 10px',
              fontSize: 12,
              borderRadius: 'var(--ol-r-sm)',
              border: '0.5px solid var(--ol-line-strong)',
              background: 'var(--ol-surface-2)',
              color: 'var(--ol-ink)',
              cursor: 'default',
              fontFamily: 'inherit',
            }}
          >
            {t('style.appOverride.addButton')}
          </button>
        </div>
        <div style={{ fontSize: 11.5, color: 'var(--ol-ink-3)', marginBottom: 14, lineHeight: 1.5 }}>
          {t('style.appOverride.desc')}
        </div>

        {overrides && overrides.length === 0 && (
          <div style={{ fontSize: 12, color: 'var(--ol-ink-3)', padding: 12 }}>
            {t('style.appOverride.empty')}
          </div>
        )}

        {overrides && overrides.length > 0 && (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
            {overrides.map((ov, idx) => (
              <div
                key={idx}
                style={{
                  display: 'grid',
                  gridTemplateColumns: '1fr 1fr auto',
                  gap: 10,
                  alignItems: 'end',
                  padding: 10,
                  background: 'var(--ol-surface-2)',
                  border: '0.5px solid var(--ol-line)',
                  borderRadius: 'var(--ol-r-md)',
                }}
              >
                <label style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
                  <span style={{ fontSize: 11, color: 'var(--ol-ink-3)' }}>
                    {t('style.appOverride.patternLabel')}
                  </span>
                  <input
                    value={ov.appPattern}
                    onChange={e => onChangeOverridePattern(idx, e.target.value)}
                    placeholder={t('style.appOverride.patternPlaceholder')}
                    style={{
                      padding: '6px 8px',
                      fontSize: 12,
                      fontFamily: 'inherit',
                      border: '0.5px solid var(--ol-line-strong)',
                      borderRadius: 'var(--ol-r-sm)',
                      background: 'var(--ol-surface)',
                      color: 'var(--ol-ink)',
                    }}
                  />
                </label>
                <label style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
                  <span style={{ fontSize: 11, color: 'var(--ol-ink-3)' }}>
                    {t('style.appOverride.modeLabel')}
                  </span>
                  <select
                    value={ov.mode}
                    onChange={e => onChangeOverrideMode(idx, e.target.value as PolishMode)}
                    style={{
                      padding: '6px 8px',
                      fontSize: 12,
                      border: '0.5px solid var(--ol-line-strong)',
                      borderRadius: 'var(--ol-r-sm)',
                      background: 'var(--ol-surface)',
                      color: 'var(--ol-ink)',
                      fontFamily: 'inherit',
                    }}
                  >
                    {BUILTIN_IDS.map(id => (
                      <option key={id} value={id}>
                        {t(`style.modes.${id}.name`)}
                      </option>
                    ))}
                    {prefs.customModes.map(cm => (
                      <option key={cm.id} value={`custom:${cm.id}`}>
                        {cm.name}
                      </option>
                    ))}
                  </select>
                </label>
                <button
                  onClick={() => onDeleteOverride(idx)}
                  aria-label={t('style.appOverride.delete')}
                  style={{
                    padding: '6px 10px',
                    fontSize: 11,
                    borderRadius: 'var(--ol-r-sm)',
                    border: '0.5px solid var(--ol-line-strong)',
                    background: 'var(--ol-surface)',
                    color: 'var(--ol-red, #ef4444)',
                    cursor: 'default',
                    fontFamily: 'inherit',
                  }}
                >
                  {t('style.appOverride.delete')}
                </button>
              </div>
            ))}
          </div>
        )}

        <div
          style={{
            marginTop: 12,
            fontSize: 11,
            color: 'var(--ol-ink-3)',
            lineHeight: 1.5,
          }}
        >
          {t('style.appOverride.hint')}
        </div>

        {overrideError && (
          <div
            role="alert"
            style={{ marginTop: 10, fontSize: 11.5, color: 'var(--ol-red, #ef4444)', lineHeight: 1.45 }}
          >
            {t('style.saveFailed', { error: overrideError })}
          </div>
        )}
      </div>
    </>
  );
}
