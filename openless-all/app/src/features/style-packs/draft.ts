import type { StylePack, StylePackExample } from '../../api/contracts';

export function clonePack(pack: StylePack): StylePack {
  return {
    ...pack,
    tags: [...pack.tags],
    examples: pack.examples.map((example) => ({ ...example })),
  };
}

export function editableFingerprint(pack: StylePack | null): string {
  if (!pack) return '';
  return JSON.stringify({
    name: pack.name,
    description: pack.description,
    author: pack.author ?? '',
    version: pack.version,
    selectionPrompt: pack.selectionPrompt,
    voiceEditPrompt: pack.voiceEditPrompt ?? '',
    prompt: pack.prompt,
    examples: pack.examples,
    tags: pack.tags,
    recommendedModel: pack.recommendedModel ?? '',
    compatibleAppVersion: pack.compatibleAppVersion ?? '',
  });
}

export function blankExample(): StylePackExample {
  return {
    title: '',
    input: '',
    output: '',
  };
}
