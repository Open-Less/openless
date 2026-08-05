import { isTranslationEnabled, isTranslationTargetRedundant } from './translationTarget';

function assert(condition: boolean, message: string) {
  if (!condition) throw new Error(message);
}

// 未选目标语言 = 功能未启用。
assert(isTranslationEnabled('') === false, 'empty target should read as disabled');
assert(isTranslationEnabled('   ') === false, 'blank target should read as disabled');
assert(isTranslationEnabled('English') === true, 'a chosen target should read as enabled');

// 目标 = 唯一工作语言：翻译是空操作，页面必须提示。
assert(
  isTranslationTargetRedundant('简体中文', ['简体中文']) === true,
  'target equal to the only working language should be flagged redundant',
);
assert(
  isTranslationTargetRedundant(' 简体中文 ', ['简体中文']) === true,
  'surrounding whitespace should not hide a redundant target',
);

// 简→繁是真实转换，不能误判成空操作。
assert(
  isTranslationTargetRedundant('繁体中文', ['简体中文']) === false,
  'simplified to traditional is a real conversion',
);

// 多工作语言不拦：说中文出英文是正常用法。
assert(
  isTranslationTargetRedundant('English', ['简体中文', 'English']) === false,
  'multiple working languages should never be flagged',
);

// 没选目标语言时走「未启用」提示，不该同时报「冗余」。
assert(
  isTranslationTargetRedundant('', ['简体中文']) === false,
  'an unset target is disabled, not redundant',
);
assert(
  isTranslationTargetRedundant('English', []) === false,
  'no working languages means nothing to compare against',
);
