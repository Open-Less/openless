// iOS 键盘扩展设置面板：扩展启用状态 + App Group 诊断 + 转写配置表单。
// 仅在 iOS（platform === 'mobile'）渲染；桌面/Android 不导入此组件的运行时分支。

import { useCallback, useEffect, useState, type CSSProperties } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../../../src/components/Icon';
import { Btn, Pill } from '../../../src/pages/_atoms';
import { SettingRow } from '../../../src/pages/settings/shared';
import {
  getIosAppGroupStatus,
  getIosKeyboardConfig,
  getIosKeyboardStatus,
  setIosKeyboardConfig,
  type IosKeyboardConfig,
} from '../lib/iosIpc';

const inputStyle: CSSProperties = {
  flex: '1 1 auto',
  minWidth: 0,
  fontSize: 12,
  padding: '6px 8px',
  border: '1px solid var(--ol-line-1)',
  borderRadius: 8,
  background: 'transparent',
  color: 'var(--ol-ink-1)',
};

export function IosKeyboardPanel() {
  const { t } = useTranslation();
  const [keyboardEnabled, setKeyboardEnabled] = useState<boolean | null>(null);
  const [appGroupOk, setAppGroupOk] = useState<boolean | null>(null);
  const [config, setConfig] = useState<IosKeyboardConfig | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [savedAt, setSavedAt] = useState<number | null>(null);

  const refreshStatus = useCallback(async () => {
    setKeyboardEnabled(
      await getIosKeyboardStatus()
        .then((s) => s.enabled)
        .catch(() => false),
    );
    setAppGroupOk(await getIosAppGroupStatus().catch(() => false));
  }, []);

  useEffect(() => {
    void refreshStatus();
    void getIosKeyboardConfig()
      .then(setConfig)
      .catch(() => setConfig({ endpoint: '', apiKey: '', model: '', prompt: '' }));
  }, [refreshStatus]);

  const save = async () => {
    if (!config) return;
    setSaving(true);
    setSaveError(null);
    try {
      await setIosKeyboardConfig(config);
      setSavedAt(Date.now());
    } catch (error) {
      setSaveError(error instanceof Error ? error.message : String(error));
    } finally {
      setSaving(false);
    }
  };

  const update = (patch: Partial<IosKeyboardConfig>) => {
    setConfig((prev) => (prev ? { ...prev, ...patch } : prev));
  };

  return (
    <>
      <SettingRow label={t('settings.iosKeyboard.extensionLabel')}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          {keyboardEnabled === null ? (
            <Pill tone="default">{t('settings.permissions.checking')}</Pill>
          ) : keyboardEnabled ? (
            <Pill tone="ok">
              <Icon name="check" size={11} />
              {t('settings.iosKeyboard.enabled')}
            </Pill>
          ) : (
            <Pill tone="outline">{t('settings.iosKeyboard.disabled')}</Pill>
          )}
          <Btn variant="ghost" size="sm" onClick={() => void refreshStatus()}>
            {t('common.retry')}
          </Btn>
        </div>
      </SettingRow>
      <SettingRow label={t('settings.iosKeyboard.appGroupLabel')}>
        {appGroupOk === null ? (
          <Pill tone="default">{t('settings.permissions.checking')}</Pill>
        ) : appGroupOk ? (
          <Pill tone="ok">
            <Icon name="check" size={11} />
            {t('settings.iosKeyboard.available')}
          </Pill>
        ) : (
          <Pill tone="outline">{t('settings.iosKeyboard.unavailable')}</Pill>
        )}
      </SettingRow>
      {config && (
        <SettingRow label={t('settings.iosKeyboard.configLabel')}>
          <div
            style={{ display: 'flex', flexDirection: 'column', gap: 6, width: '100%', minWidth: 0 }}
          >
            <input
              style={inputStyle}
              placeholder={t('settings.iosKeyboard.endpointPlaceholder')}
              value={config.endpoint}
              onChange={(e) => update({ endpoint: e.target.value })}
            />
            <input
              style={inputStyle}
              type="password"
              placeholder={t('settings.iosKeyboard.keyPlaceholder')}
              value={config.apiKey}
              onChange={(e) => update({ apiKey: e.target.value })}
            />
            <input
              style={inputStyle}
              placeholder={t('settings.iosKeyboard.modelPlaceholder')}
              value={config.model}
              onChange={(e) => update({ model: e.target.value })}
            />
            <input
              style={inputStyle}
              placeholder={t('settings.iosKeyboard.promptPlaceholder')}
              value={config.prompt}
              onChange={(e) => update({ prompt: e.target.value })}
            />
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <Btn
                size="sm"
                onClick={() => void save()}
                disabled={saving || !config.endpoint || !config.apiKey || !config.model}
              >
                {t('settings.iosKeyboard.save')}
              </Btn>
              {savedAt !== null && !saveError && (
                <span style={{ fontSize: 11, color: 'var(--ol-ink-3)' }}>
                  {t('settings.iosKeyboard.saved')}
                </span>
              )}
              {saveError && (
                <span style={{ fontSize: 11, color: 'var(--ol-danger-1, #c0392b)' }}>
                  {saveError}
                </span>
              )}
            </div>
            <span style={{ fontSize: 11, color: 'var(--ol-ink-4)' }}>
              {t('settings.iosKeyboard.hint')}
            </span>
          </div>
        </SettingRow>
      )}
    </>
  );
}
