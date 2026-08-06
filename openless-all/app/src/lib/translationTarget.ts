// 翻译目标语言的可用性判定，与后端 `types.rs::translation_effective` 保持同一套规则。
// 后端在按下翻译修饰键时用它决定是否进入翻译管线；这里只负责在翻译页提前把「设了但
// 不会生效」的组合告诉用户，避免出现「按了 Shift 却什么也没翻」的沉默失败。

/** 未选择目标语言 = 翻译功能未启用（Shift 无效）。 */
export function isTranslationEnabled(translationTargetLanguage: string): boolean {
  return translationTargetLanguage.trim() !== '';
}

/**
 * 目标语言与用户「唯一的」工作语言相同 —— 源语言必定就是目标语言，翻译是可证的空操作。
 *
 * 工作语言有多个时返回 false：中/英双语用户把目标设成英文是正常用法（说中文出英文），
 * 源语言无法预先判定，不能拦。简体/繁体是语言列表里两个独立条目，按字面比较即可，
 * 简→繁不会被误判成空操作。
 */
export function isTranslationTargetRedundant(
  translationTargetLanguage: string,
  workingLanguages: readonly string[],
): boolean {
  const target = translationTargetLanguage.trim();
  if (target === '') return false;
  return workingLanguages.length === 1 && workingLanguages[0].trim() === target;
}
