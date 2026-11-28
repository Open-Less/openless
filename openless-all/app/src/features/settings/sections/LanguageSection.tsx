// UI language is persisted separately; it never overwrites working languages or the translation target.

import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  FOLLOW_SYSTEM,
  getLocalePreference,
  setLocalePreference,
  type SupportedLocale,
} from '../../../i18n/index';
import { InterfaceLanguagePicker } from '../../../ui/controls/InterfaceLanguagePicker';
import { Card } from '../../../ui/layout/Card';
import { SectionTitle } from '../../../ui/layout/SectionTitle';
import { SettingRow } from '../../../ui/controls/SettingRow';

export function LanguageSection() {
  const { t, i18n } = useTranslation();
  const [pref, setPref] = useState<SupportedLocale | typeof FOLLOW_SYSTEM>(getLocalePreference());

  useEffect(() => setPref(getLocalePreference()), [i18n.resolvedLanguage]);

  const apply = async (next: SupportedLocale | typeof FOLLOW_SYSTEM) => {
    setPref(next);
    // Interface language is independent from speech recognition and translation targets.
    await setLocalePreference(next);
  };

  return (
    <Card>
      <SectionTitle>{t('settings.language.title')}</SectionTitle>
      <SettingRow label={t('settings.language.label')}>
        <InterfaceLanguagePicker
          value={pref}
          onChange={apply}
          ariaLabel={t('settings.language.label')}
        />
      </SettingRow>
    </Card>
  );
}
