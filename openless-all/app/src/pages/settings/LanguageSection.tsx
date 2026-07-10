// 语言切换面板：跟随系统 / 简中 / 繁中 / 英文 / 日文 (Beta) / 韩文 (Beta)。
// 切换语言同时把对应的 outputPrefs（中文偏好、输出语言）合并进 prefs。

import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import {
  FOLLOW_SYSTEM,
  getLocalePreference,
  outputPrefsForLocale,
  setLocalePreference,
  type SupportedLocale,
} from '../../i18n';
import { SelectLite } from '../../components/ui/SelectLite';
import { Card } from '../_atoms';
import { SettingRow } from './shared';

export function LanguageSection() {
  const { t } = useTranslation();
  const { updatePrefs } = useHotkeySettings();
  const [pref, setPref] = useState<SupportedLocale | typeof FOLLOW_SYSTEM>(getLocalePreference());

  const options = useMemo(() => ([
    { value: FOLLOW_SYSTEM, label: t('settings.language.followSystem') },
    { value: 'zh-CN', label: t('settings.language.zh') },
    { value: 'zh-TW', label: t('settings.language.zhTW') },
    { value: 'en', label: t('settings.language.en') },
    { value: 'ja', label: t('settings.language.ja') },
    { value: 'ko', label: t('settings.language.ko') },
  ]), [t]);

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
      <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 6 }}>{t('settings.language.title')}</div>
      <SettingRow label={t('settings.language.label')}>
        <SelectLite
          value={pref}
          onChange={next => apply(next as SupportedLocale | typeof FOLLOW_SYSTEM)}
          options={options}
          ariaLabel={t('settings.language.label')}
          style={{ maxWidth: 220, minWidth: 200 }}
        />
      </SettingRow>
    </Card>
  );
}
