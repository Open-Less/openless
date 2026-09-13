import { createInstance } from 'i18next';
import { zhCN } from '../../i18n/zh-CN';
import { zhTW } from '../../i18n/zh-TW';
import { en } from '../../i18n/en';
import { ja } from '../../i18n/ja';
import { ko } from '../../i18n/ko';
import { ADVANCED_PAGES, SETTINGS_SECTIONS, searchSettingsSections } from './navigation';

// Resources must resolve where the dialog actually reads them, not in a nested
// feature's unrelated "modal" object. Type-shape parity alone cannot detect this.
for (const [locale, translation] of Object.entries({ 'zh-CN': zhCN, 'zh-TW': zhTW, en, ja, ko })) {
  const i18n = createInstance();
  await i18n.init({ lng: locale, fallbackLng: false, resources: { [locale]: { translation } } });
  for (const key of [
    'modal.backToAdvanced',
    ...ADVANCED_PAGES.flatMap((page) => [page.titleKey, `modal.advancedPages.${page.id}`]),
  ]) {
    if (!i18n.exists(key)) throw new Error(`${locale}: missing advanced navigation label ${key}`);
  }
  for (const key of [
    'modal.searchPlaceholder',
    'modal.searchResults',
    'modal.clearSearch',
    'modal.noResults',
    'startup.failed',
    'startup.retry',
    ...['label', 'llm', 'asr', 'omni', 'models', 'connections'].map(
      (view) => `modal.serviceViews.${view}`,
    ),
  ]) {
    if (!i18n.exists(key)) throw new Error(`${locale}: missing visible label ${key}`);
  }
  const sections = SETTINGS_SECTIONS.map((item) => {
    for (const prefix of ['sections', 'descriptions', 'searchKeywords']) {
      if (!i18n.exists(`modal.${prefix}.${item.id}`))
        throw new Error(`${locale}: missing ${prefix} for ${item.id}`);
    }
    return {
      ...item,
      title: i18n.t(`modal.sections.${item.id}`),
      description: i18n.t(`modal.descriptions.${item.id}`),
      keywords: i18n.t(`modal.searchKeywords.${item.id}`),
    };
  });
  if (searchSettingsSections(sections, 'API')[0]?.id !== 'services')
    throw new Error(`${locale}: API search cannot reach service configuration`);
  if (!i18n.t('modal.searchCount', { count: 2 }).includes('2'))
    throw new Error(`${locale}: result count is not rendered`);
}
console.log('settings localization and search routing passed in five languages');
