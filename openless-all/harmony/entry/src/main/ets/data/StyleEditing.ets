import { MarketplacePack, StylePack } from './Types';

/** Core update_style_pack replaces behavior fields. Never send a UI-only DTO. */
export class StyleEditing {
  static create(name: string, prompt: string): StylePack {
    const trimmed: string = name.trim();
    if (trimmed.length === 0) throw new Error('请输入风格名称');
    return {
      id: '', name: trimmed, description: '', author: null, version: '1.0.0',
      kind: 'imported', baseMode: 'light', prompt: prompt,
      selectionPrompt: '', voiceEditPrompt: '', examples: [], tags: [],
      iconPath: null, createdAt: null, updatedAt: null, enabled: true, active: false,
      recommendedModel: null, compatibleAppVersion: null,
      originPackId: null, originAuthorLogin: null
    };
  }

  static withPrompt(original: StylePack, prompt: string): StylePack {
    // Fail closed if an old/partial adapter value cannot preserve Core's fields.
    if (typeof original.id !== 'string' || original.id.length === 0 ||
      typeof original.name !== 'string' || typeof original.description !== 'string' ||
      typeof original.version !== 'string' || typeof original.prompt !== 'string' ||
      typeof original.selectionPrompt !== 'string' || typeof original.voiceEditPrompt !== 'string' ||
      !Array.isArray(original.examples) || !Array.isArray(original.tags) ||
      (original.author !== null && typeof original.author !== 'string') ||
      (original.recommendedModel !== null && typeof original.recommendedModel !== 'string') ||
      (original.compatibleAppVersion !== null && typeof original.compatibleAppVersion !== 'string')) {
      throw new Error('风格详情不完整，请刷新后重试，未保存任何修改');
    }
    // Clone the full wire object so even future metadata fields survive this editor.
    const edited: StylePack = JSON.parse(JSON.stringify(original)) as StylePack;
    edited.prompt = prompt;
    return edited;
  }

  static installed(pack: MarketplacePack, local: StylePack[]): boolean {
    return local.some((item: StylePack) => item.originPackId === pack.id &&
      item.originAuthorLogin === pack.authorLogin);
  }
}
