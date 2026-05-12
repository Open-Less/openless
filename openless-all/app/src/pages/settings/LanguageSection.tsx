// 语言切换面板：跟随系统 / 简中 / 繁中 / 英文 / 日文 (Beta) / 韩文 (Beta)。
// 切换语言同时把对应的 outputPrefs（中文偏好、输出语言）合并进 prefs。

import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import {
  FOLLOW_SYSTEM,
  getLocalePreference,
  outputPrefsForLocale,
  setLocalePreference,
  type SupportedLocale,
} from '../../i18n';
import { Card } from '../_atoms';
import { SettingRow, inputStyle } from './shared';

export function LanguageSection() {
  const { t } = useTranslation();
  const { updatePrefs } = useHotkeySettings();
  const [pref, setPref] = useState<SupportedLocale | typeof FOLLOW_SYSTEM>(getLocalePreference());

  const apply = async (next: SupportedLocale | typeof FOLLOW_SYSTEM) => {
    setPref(next);
    const resolved = await setLocalePreference(next);
    const localePrefs = outputPrefsForLocale(resolved);
    await updatePrefs(current => {
      if (
        current.chineseScriptPreference === localePrefs.chineseScriptPreference &&
        current.outputLanguagePreference === localePrefs.outputLanguagePreference
      ) {
        return current;
      }
      return { ...current, ...localePrefs };
    });
  };

  return (
    <Card>
      <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 4 }}>{t('settings.language.title')}</div>
      <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginBottom: 6 }}>{t('settings.language.desc')}</div>
      <SettingRow label={t('settings.language.label')} desc={t('settings.language.labelDesc')}>
        <select
          value={pref}
          onChange={e => apply(e.target.value as SupportedLocale | typeof FOLLOW_SYSTEM)}
          style={{ ...inputStyle, maxWidth: 220 }}
        >
          <option value={FOLLOW_SYSTEM}>{t('settings.language.followSystem')}</option>
          <option value="zh-CN">{t('settings.language.zh')}</option>
          <option value="zh-TW">{t('settings.language.zhTW')}</option>
          <option value="en">{t('settings.language.en')}</option>
          <option value="ja">{t('settings.language.ja')}</option>
          <option value="ko">{t('settings.language.ko')}</option>
        </select>
      </SettingRow>
      <div style={{ fontSize: 11, color: 'var(--ol-ink-4)', marginTop: 8, lineHeight: 1.6 }}>
        {t('settings.language.restartHint')}
      </div>
    </Card>
  );
}
