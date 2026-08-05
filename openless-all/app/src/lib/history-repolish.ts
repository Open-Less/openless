import type { DictationSession, PolishMode, StylePack } from './types';

/**
 * 「用原风格重试」要用的风格包 id。
 *
 * 优先取产生这条记录的风格包（session.stylePackId）——重试的目的是跟上次结果做
 * A/B 对照，必须用同一套风格，否则判断不了是模型抖动还是风格差异。包已被删除、
 * 旧历史没有 stylePackId、或顶层包列表尚未加载（allPacks 为 null）时返回 undefined，
 * 由调用方回落当前激活风格包（repolish 省略 stylePackId 的行为）。
 *
 * 注意查的是 allPacks（含已禁用包）：历史可能出自后来被禁用的包，只要包还在就能重试。
 */
export function resolveRepolishRetryPackId(
  session: Pick<DictationSession, 'stylePackId'>,
  allPacks: StylePack[] | null,
): string | undefined {
  if (!session.stylePackId || !allPacks) return undefined;
  return allPacks.some(pack => pack.id === session.stylePackId)
    ? session.stylePackId
    : undefined;
}

/**
 * 风格包在界面上的显示名。
 *
 * 内置包例外：后端内置包名是硬编码中文（"轻度润色"…），直接显示会在英/日/韩界面
 * 串语言，所以内置包一律走 i18n 的 mode 名（与历史条目 Pill 同一原则）。自定义包
 * 显示用户起的原名。
 */
export function packDisplayName(
  pack: StylePack,
  modeLabel: Record<PolishMode, string>,
): string {
  return pack.kind === 'builtin' ? modeLabel[pack.baseMode] : pack.name.trim();
}

/** 「换风格」下拉的默认选中项：当前激活包优先，其次第一个可用包，空列表返回 ''。 */
export function defaultPackId(packs: StylePack[]): string {
  return packs.find(pack => pack.active)?.id || packs[0]?.id || '';
}

/**
 * 「用原风格重试」实际要用的风格包 id：优先产生这条记录的原包；原包已删除、旧历史
 * 没有 stylePackId、或包列表尚未加载时，显式落到当前激活包（其次第一个可用包）——
 * 显式传 id 让前端标注与实际执行一致，而不是让后端走 None 的兜底链。
 */
export function resolveRepolishRetryPackIdWithFallback(
  session: Pick<DictationSession, 'stylePackId'>,
  allPacks: StylePack[] | null,
  enabledPacks: StylePack[],
): string | undefined {
  return (resolveRepolishRetryPackId(session, allPacks) ?? defaultPackId(enabledPacks)) || undefined;
}
