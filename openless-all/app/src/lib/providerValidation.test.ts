import { createInstance } from 'i18next';
import { providerValidationCopy } from './providerValidation';
import { zhCN } from '../i18n/zh-CN';
import { zhTW } from '../i18n/zh-TW';
import { en } from '../i18n/en';
import { ja } from '../i18n/ja';
import { ko } from '../i18n/ko';
import { es } from '../i18n/es';
import { fr } from '../i18n/fr';
import { de } from '../i18n/de';

const realtime = providerValidationCopy('omni', 'openai-realtime');
if (
  realtime.hintKey !== 'settings.channels.validationHintRealtime' ||
  realtime.successKey !== 'settings.providers.validateSuccessRealtime'
)
  throw new Error('Realtime validation must not claim a generated text or audio test');
const textOmni = providerValidationCopy('omni', 'openai');
if (
  textOmni.hintKey !== 'settings.channels.validationHintOmni' ||
  textOmni.successKey !== 'settings.providers.validateSuccessOmni'
)
  throw new Error('Existing OpenAI Chat validation must keep its text-only description');
if (
  providerValidationCopy('omni', 'gemini', true).successKey !==
  'settings.providers.validateSuccessAudio'
)
  throw new Error('Gemini transcription audio checks must preserve their audio test description');
if (providerValidationCopy('llm', 'openai').successKey !== 'settings.providers.validateSuccess')
  throw new Error('Traditional text-provider checks must remain unchanged');

for (const [locale, translation] of Object.entries({
  'zh-CN': zhCN,
  'zh-TW': zhTW,
  en,
  ja,
  ko,
  es,
  fr,
  de,
})) {
  const instance = createInstance();
  await instance.init({
    lng: locale,
    fallbackLng: false,
    resources: { [locale]: { translation } },
  });
  for (const key of [
    realtime.hintKey,
    realtime.successKey,
    'settings.providers.openaiRealtimeOmniNotice',
    'settings.providers.presets.omniOpenaiRealtime',
  ]) {
    if (!instance.exists(key) || !instance.t(key).trim())
      throw new Error(`${locale}: native Realtime configuration copy is missing: ${key}`);
  }
}
console.log(
  'Realtime handshake copy and all eight locales distinguish it from generated validation',
);
