import { useTranslation } from 'react-i18next';
import { FOLLOW_SYSTEM, SUPPORTED_LOCALES, type SupportedLocale } from '../../i18n';
import { SelectLite } from './SelectLite';

type LocalePreference = SupportedLocale | typeof FOLLOW_SYSTEM;

const labelKeys: Record<SupportedLocale, string> = {
  'zh-CN': 'zh',
  'zh-TW': 'zhTW',
  en: 'en',
  ja: 'ja',
  ko: 'ko',
  es: 'es',
  fr: 'fr',
  de: 'de',
};

export function InterfaceLanguagePicker({
  value,
  onChange,
  ariaLabel,
  side,
  includeSystem = true,
}: {
  value: string;
  onChange: (next: LocalePreference) => void;
  ariaLabel: string;
  side?: 'top' | 'bottom';
  includeSystem?: boolean;
}) {
  const { t } = useTranslation();
  const options: { value: LocalePreference; label: string }[] = SUPPORTED_LOCALES.map((locale) => ({
    value: locale,
    label: t(`settings.language.${labelKeys[locale]}`),
  }));
  if (includeSystem)
    options.unshift({ value: FOLLOW_SYSTEM, label: t('settings.language.followSystem') });

  return (
    <SelectLite
      value={value}
      options={options}
      onChange={(next) => {
        const option = options.find(({ value }) => value === next);
        if (option) onChange(option.value);
      }}
      ariaLabel={ariaLabel}
      side={side}
      searchable
      searchPlaceholder={t('translation.searchLanguages')}
      emptyMessage={t('translation.noMatchingLanguages')}
    />
  );
}
