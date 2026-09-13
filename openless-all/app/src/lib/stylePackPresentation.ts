import type { TFunction } from 'i18next';
import type { PolishMode, StylePack } from './types';

type DisplayPack = Pick<StylePack, 'id' | 'kind' | 'baseMode' | 'name' | 'description'> &
  Partial<Pick<StylePack, 'tags'>>;

const DEFAULT_NAMES: Record<PolishMode, string> = {
  raw: '原文',
  light: '轻度润色',
  structured: '清晰结构',
  formal: '正式表达',
};

// Match shipped Core metadata and browser-preview defaults exactly. A translated
// card must never overwrite a user's renamed pack or edited description.
const DEFAULT_DESCRIPTIONS: Record<PolishMode, readonly string[]> = {
  raw: [
    '尽量保留原话的顺序、语气和信息密度，只做必要断句与标点整理。',
    '尽量保留原话顺序和语气，只做必要的断句与标点整理。',
  ],
  light: [
    '在保留原意 / 语气 / 表达习惯前提下，把口语转写整理成自然顺畅、可直接发送或继续编辑的文字。v2.0 中文序号七节骨架（角色 → 核心原则 → 润色强度 → 风格判断 → ASR 纠错 → 原样保留 → 禁止事项 → 输出），把「± 20% 字数」「工程化直陈 vs 自然润色」两个判断点抽到独立章节作为最显眼的两个开关。',
    '把口述整理成顺畅、自然、可直接发送的文字，不扩写事实。',
  ],
  structured: [
    '面向 AI 编程协作、技术排障、模型资讯和产品 UI 反馈，优先保证术语与结构准确。v3.0 Beta：人格化「语修」角色 + 场景优先级分型 + ASR 术语纠错词表 + 反 AI 自述式表达约束，双层格式与锚示例保持不变。',
    '适合多事项和多主题口述，自动整理为层次清楚的结构化输出。',
    '面向 AI 编程协作、技术排障和模型资讯，优先保证术语与结构准确。',
  ],
  formal: [
    '把口语转写整理成适合工作沟通、邮件、跨团队同步的正式书面表达。v2.0 中文序号七节骨架（角色 → 核心原则 → 正式化强度 → 风格判断 → ASR 纠错 → 原样保留 → 禁止事项 → 输出），把「± 30% 字数」「通用商务正式 vs 邮件场景识别问候落款」两个判断点抽到独立章节；含邮件场景示例覆盖问候/落款识别规则。',
    '适合邮件、同步和工作沟通场景，语气更完整、专业、克制。',
  ],
};

const DEFAULT_TAG_KEYS: Record<PolishMode, Readonly<Record<string, string>>> = {
  raw: {
    原文: 'style.modes.raw.name',
    最小改写: 'style.pack.builtinTags.minimalEdits',
  },
  light: {
    轻度润色: 'style.modes.light.name',
    强纠错: 'style.pack.builtinTags.strongCorrection',
    沟通: 'style.pack.builtinTags.communication',
    自然: 'style.pack.builtinTags.natural',
  },
  structured: {
    'AI 编程': 'style.pack.builtinTags.aiCoding',
    技术结构化: 'style.pack.builtinTags.technicalStructure',
    结构化: 'style.modes.structured.name',
    条理: 'style.pack.builtinTags.organized',
  },
  formal: {
    正式表达: 'style.modes.formal.name',
    正式: 'style.modes.formal.name',
    强纠错: 'style.pack.builtinTags.strongCorrection',
    工作沟通: 'style.pack.builtinTags.workplaceCommunication',
  },
};

function isBuiltin(pack: Pick<DisplayPack, 'id' | 'kind' | 'baseMode'>): boolean {
  return pack.kind === 'builtin' && pack.id === `builtin.${pack.baseMode}`;
}

export function stylePackDisplayName(
  pack: Pick<DisplayPack, 'id' | 'kind' | 'baseMode' | 'name'>,
  modeLabels: Record<PolishMode, string>,
): string {
  const name = pack.name.trim();
  return isBuiltin(pack) && name === DEFAULT_NAMES[pack.baseMode]
    ? modeLabels[pack.baseMode]
    : name;
}

/** Localized labels for display only. Persistence and export keep original data. */
export function getStylePackPresentation(
  pack: DisplayPack,
  t: TFunction,
): {
  name: string;
  description: string;
  tags: string[];
} {
  const name = stylePackDisplayName(pack, {
    raw: t('style.modes.raw.name'),
    light: t('style.modes.light.name'),
    structured: t('style.modes.structured.name'),
    formal: t('style.modes.formal.name'),
  });
  const description = pack.description.trim();
  return {
    name,
    description:
      isBuiltin(pack) && DEFAULT_DESCRIPTIONS[pack.baseMode].includes(description)
        ? t(`style.modes.${pack.baseMode}.desc`)
        : description,
    tags: (pack.tags ?? []).map((tag) => {
      const key = isBuiltin(pack) ? DEFAULT_TAG_KEYS[pack.baseMode][tag] : undefined;
      return key ? t(key) : tag;
    }),
  };
}
