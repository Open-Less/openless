import { createInstance } from 'i18next';
import { withDeletionMessages } from './deletionMessages';
import { en } from './en';
import { ja } from './ja';
import { zhCN } from './zh-CN';
import { zhTW } from './zh-TW';

const i18n = createInstance();
await i18n.init({
  lng: 'en',
  fallbackLng: false,
  resources: {
    en: { translation: withDeletionMessages(en, 'en') },
    'zh-CN': { translation: withDeletionMessages(zhCN, 'zh-CN') },
    'zh-TW': { translation: withDeletionMessages(zhTW, 'zh-TW') },
    ja: { translation: withDeletionMessages(ja, 'ja') },
  },
  interpolation: { escapeValue: false },
});

const examples = {
  'history.confirmClearLocal': [],
  'history.confirmDeleteLocal': [],
  'history.cleared': [],
  'history.deleted': [],
  'usage.clear': [],
  'usage.clearScope': [],
  'usage.clearFailed': [],
  'usage.cleared': [],
  'vocab.deleteWord': ['phrase'],
  'vocab.deleteLearned': ['count'],
  'vocab.deleteScope': [],
  'vocab.deleteIncomplete': ['count'],
  'vocab.deleted': [],
  'vocab.corrections.deleteLearnedTarget': ['count'],
  'vocab.corrections.deleteScope': ['target'],
  'vocab.corrections.deleteIncomplete': ['count'],
  'vocab.corrections.deleted': [],
  'style.pack.resetBuiltinScope': ['name'],
  'style.pack.deleteLocalScope': ['name'],
  'marketplace.detail.withdrawScope': ['name'],
  'settings.expressionLearning.clearScope': [],
  'settings.channels.deleteScope': ['name'],
  'localAsr.cleanupScope': ['name'],
  'localAsr.cleanupBatchScope': ['count'],
  'localAsr.cleanupIncompleteError': ['count'],
} as const;
const params = { phrase: 'Fixture & word', target: 'x → y', name: 'Fixture & name', count: 9 };
for (const [key, variables] of Object.entries(examples)) {
  const english = i18n.getFixedT('en')(key, params);
  if (english === key || /\p{Script=Han}/u.test(english)) throw new Error(`Missing English: ${key}`);
  if (i18n.getFixedT('ja')(key, params) !== english) throw new Error(`Changed fallback: ${key}`);
  for (const locale of ['en', 'zh-CN', 'zh-TW'] as const) {
    const message = i18n.getFixedT(locale)(key, params);
    if (locale !== 'en' && !/\p{Script=Han}/u.test(message)) {
      throw new Error(`Missing ${locale}: ${key}`);
    }
    for (const variable of variables) {
      if (!message.includes(String(params[variable]))) {
        throw new Error(`${locale}.${key} must interpolate ${variable}`);
      }
    }
  }
}

if (i18n.getFixedT('zh-TW')('history.deleted') !== '已刪除記錄') {
  throw new Error('Traditional Chinese must use its own deletion copy');
}
if (i18n.getFixedT('ja')('common.cancel') !== ja.common.cancel) {
  throw new Error('Deletion fallbacks must leave existing locale messages in place');
}
