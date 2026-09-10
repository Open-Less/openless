import { zhCN } from './zh-CN';
import { zhTW } from './zh-TW';
import { en } from './en';
import { ja } from './ja';
import { ko } from './ko';

const locales = { zhCN, zhTW, en, ja, ko };

for (const [locale, messages] of Object.entries(locales)) {
  const marker = messages.common.experimental;
  if (!marker) throw new Error(`${locale} must define the shared experimental badge`);

  for (const [key, title] of Object.entries({
    multimodalPipelineTitle: messages.settings.advanced.multimodalPipelineTitle,
    localAsrTitle: messages.settings.advanced.localAsrTitle,
  })) {
    if (title.includes(marker)) {
      throw new Error(
        `${locale}.${key} must leave the experimental marker to the shared title component`,
      );
    }
  }

  if (messages.localAsr.qwenExperimentalBadge !== marker) {
    throw new Error(`${locale} Qwen badge must use the same experimental wording`);
  }

  for (const [key, title] of Object.entries({
    streamingInsertTitleLinux: messages.settings.advanced.streamingInsertTitleLinux,
    asrSherpaOnnxLocal: messages.settings.providers.presets.asrSherpaOnnxLocal,
    sherpaTitle: messages.localAsr.sherpaTitle,
  })) {
    if (!title.includes(marker)) {
      throw new Error(`${locale}.${key} must use the shared experimental wording`);
    }
  }
}

console.log('experimental title translations are consistent');
