import { de } from './de';
import { en } from './en';
import { es } from './es';
import { fr } from './fr';
import { ja } from './ja';
import { ko } from './ko';
import { zhCN } from './zh-CN';
import { zhTW } from './zh-TW';

for (const [locale, messages] of Object.entries({ de, en, es, fr, ja, ko, zhCN, zhTW })) {
  const learning = messages.settings.vocabularyLearning;
  for (const key of [
    'suggestionPosition',
    'suggestionPositionHint',
    'positionNearInput',
    'positionBottomRight',
  ] as const) {
    if (!learning[key].trim()) {
      throw new Error(`${locale}: missing vocabulary learning ${key}`);
    }
  }
  if (learning.positionNearInput === learning.positionBottomRight) {
    throw new Error(`${locale}: vocabulary position choices need distinct labels`);
  }
  if (!learning.suggestionPositionHint.includes('Windows')) {
    throw new Error(`${locale}: vocabulary position hint must state the Windows-only scope`);
  }
}

console.log('vocabulary learning position translations cover both Windows choices');
