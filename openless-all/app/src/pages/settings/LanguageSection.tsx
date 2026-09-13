// 界面语言单独持久化；不覆盖工作语言或翻译目标。

import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  FOLLOW_SYSTEM,
  getLocalePreference,
  setLocalePreference,
  type SupportedLocale,
} from '../../i18n';
import { SelectLite } from '../../components/ui/SelectLite';
import { Card } from '../_atoms';
import { SettingRow } from './shared';

export function LanguageSection() {
  const { t, i18n } = useTranslation();
  const [pref, setPref] = useState<SupportedLocale | typeof FOLLOW_SYSTEM>(getLocalePreference());

  useEffect(() => setPref(getLocalePreference()), [i18n.resolvedLanguage]);

  const options = useMemo(
    () => [
      { value: FOLLOW_SYSTEM, label: t('settings.language.followSystem') },
      { value: 'zh-CN', label: t('settings.language.zh') },
      { value: 'zh-TW', label: t('settings.language.zhTW') },
      { value: 'en', label: t('settings.language.en') },
      { value: 'ja', label: t('settings.language.ja') },
      { value: 'ko', label: t('settings.language.ko') },
      { value: 'es', label: t('settings.language.es') },
      { value: 'fr', label: t('settings.language.fr') },
      { value: 'de', label: t('settings.language.de') },
    ],
    [t],
  );

  const apply = async (next: SupportedLocale | typeof FOLLOW_SYSTEM) => {
    setPref(next);
    // Interface language is independent from speech recognition and translation targets.
    await setLocalePreference(next);
  };

  return (
    <Card>
      <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 6 }}>
        {t('settings.language.title')}
      </div>
      <SettingRow label={t('settings.language.label')}>
        <SelectLite
          value={pref}
          onChange={(next) => apply(next as SupportedLocale | typeof FOLLOW_SYSTEM)}
          options={options}
          ariaLabel={t('settings.language.label')}
          style={{ maxWidth: 220, minWidth: 200 }}
        />
      </SettingRow>
    </Card>
  );
}
