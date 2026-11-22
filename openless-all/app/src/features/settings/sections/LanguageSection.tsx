// UI language is persisted separately; it never overwrites working languages or the translation target.

import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  FOLLOW_SYSTEM,
  getLocalePreference,
  setLocalePreference,
  type SupportedLocale,
} from '../../../i18n/index';
import { SelectLite } from '../../../ui/controls/SelectLite';
import { Card } from '../../../ui/layout/Card';
import { SectionTitle } from '../../../ui/layout/SectionTitle';
import { SettingRow } from '../../../ui/controls/SettingRow';

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
      <SectionTitle>{t('settings.language.title')}</SectionTitle>
      <SettingRow label={t('settings.language.label')}>
        <SelectLite
          value={pref}
          onChange={(next) => apply(next as SupportedLocale | typeof FOLLOW_SYSTEM)}
          options={options}
          ariaLabel={t('settings.language.label')}
          searchable
          searchPlaceholder={t('modal.searchPlaceholder')}
        />
      </SettingRow>
    </Card>
  );
}
