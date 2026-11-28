import { zhCN } from './zh-CN';
import { zhTW } from './zh-TW';
import { en } from './en';
import { de } from './de';
import { es } from './es';
import { fr } from './fr';
import { ja } from './ja';
import { ko } from './ko';

const locales = { 'zh-CN': zhCN, 'zh-TW': zhTW, en, de, es, fr, ja, ko };
const settingsKeys = ['enabledDesc', 'enabledLabel'];
const noticeKeys = ['captureFailed', 'tooLarge'];

for (const [locale, resource] of Object.entries(locales)) {
  const settings = resource.settings.dictationReferences;
  const notices = resource.capsule.referenceNotice;
  if (JSON.stringify(Object.keys(settings).sort()) !== JSON.stringify(settingsKeys)) {
    throw new Error(`${locale}: dictation reference settings keys differ`);
  }
  if (JSON.stringify(Object.keys(notices).sort()) !== JSON.stringify(noticeKeys)) {
    throw new Error(`${locale}: dictation reference notice keys differ`);
  }
  for (const text of [
    ...Object.values(settings), ...Object.values(notices),
    resource.capsule.references, resource.settings.advancedPages.dictationReferencesTitle,
  ]) {
    if (typeof text !== 'string' || !text.trim()) throw new Error(`${locale}: missing reference label`);
  }
  const countPlaceholders = resource.capsule.references.match(/\{\{[^{}]+\}\}/g);
  if (JSON.stringify(countPlaceholders) !== '["{{count}}"]') {
    throw new Error(`${locale}: attachment count must interpolate count exactly once`);
  }
  console.log(`dictation references i18n ${locale}: 6 keys passed`);
}
