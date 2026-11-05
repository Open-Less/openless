import { createInstance } from 'i18next';
import { zhCN } from '../../i18n/zh-CN';
import { zhTW } from '../../i18n/zh-TW';
import { en } from '../../i18n/en';
import { ja } from '../../i18n/ja';
import { ko } from '../../i18n/ko';
import { de } from '../../i18n/de';
import { fr } from '../../i18n/fr';
import { es } from '../../i18n/es';
import { ADVANCED_PAGES, SETTINGS_SECTIONS, searchSettingsSections } from './navigation';

const retiredServiceNames = [
  'AI 服务与模型',
  'AI 服務與模型',
  'AI services & models',
  'AI サービスとモデル',
  'AI 서비스 및 모델',
  'KI-Dienste und Modelle',
  'Services et modèles d’IA',
  'Servicios y modelos de IA',
  'AI 模型、AI 与模型',
];

// Resources must resolve where the dialog actually reads them, not in a nested
// feature's unrelated "modal" object. Type-shape parity alone cannot detect this.
for (const [locale, translation] of Object.entries({
  'zh-CN': zhCN,
  'zh-TW': zhTW,
  en,
  ja,
  ko,
  de,
  fr,
  es,
})) {
  const i18n = createInstance();
  await i18n.init({ lng: locale, fallbackLng: false, resources: { [locale]: { translation } } });
  for (const key of [
    'modal.backToAdvanced',
    'modal.backToTestingFeatures',
    ...['title', 'enable', 'description', 'unavailable'].map(
      (key) => `settings.testingFeatures.${key}`,
    ),
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
    ...['label', 'llm', 'asr', 'omni', 'models', 'connections', 'inactive', 'inactiveDetail'].map(
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
  const ordered = sections.map((item) => item.id);
  if (ordered.indexOf('cloudServices') !== ordered.indexOf('services') + 1)
    throw new Error(`${locale}: cloud services is not directly after AI`);
  if (ordered[ordered.indexOf('cloudServices') + 1] !== 'appearance')
    throw new Error(`${locale}: cloud services is not directly before appearance`);
  const serviceTitle = i18n.t('modal.sections.services');
  if (
    retiredServiceNames.includes(serviceTitle) ||
    retiredServiceNames.includes(i18n.t('overview.actions.services'))
  )
    throw new Error(`${locale}: old AI services name remains`);
  if (!serviceTitle.trim() || serviceTitle === i18n.t('modal.sections.cloudServices'))
    throw new Error(`${locale}: AI and cloud service names are missing or identical`);
  for (const group of ['recording', 'text', 'diagnostics']) {
    if (!i18n.exists(`settings.advancedGroups.${group}`))
      throw new Error(`${locale}: missing experiments group ${group}`);
  }
  for (const key of [
    'settings.recording.captureGroupTitle',
    'settings.recording.capsuleGroupTitle',
    'settings.recording.recognitionGroupTitle',
    'settings.recording.feedbackGroupTitle',
  ]) {
    if (!i18n.exists(key)) throw new Error(`${locale}: missing ${key}`);
  }
  if (searchSettingsSections(sections, 'API')[0]?.id !== 'services')
    throw new Error(`${locale}: API search cannot reach service configuration`);
  if (!i18n.t('modal.searchCount', { count: 2 }).includes('2'))
    throw new Error(`${locale}: result count is not rendered`);
}
console.log('settings localization and search routing passed in eight languages');
