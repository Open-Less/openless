import { Store } from '../common/Store';

/**
 * Navigation model.
 *
 * One level only: 概览 / 历史 / 风格 / 词典 / 设置. Everything else (风格市场,
 * 翻译, 问答, 速记, 纠正规则, 模型设置, 凭据) lives inside 设置, and each content
 * page links to the tool that belongs to it - a HarmonyOS app keeps the tab bar
 * or side bar to a handful of destinations instead of listing every screen.
 */
export interface NavItem {
  id: string;
  label: string;
  group: string;
}

export class Nav {
  static readonly OVERVIEW: string = 'overview';
  static readonly HISTORY: string = 'history';
  static readonly STYLE: string = 'style';
  static readonly VOCAB: string = 'vocab';
  static readonly SETTINGS: string = 'settings';

  // Secondary destinations, reachable from 设置 and from their owning page.
  static readonly MARKETPLACE: string = 'marketplace';
  static readonly TRANSLATION: string = 'translation';
  static readonly QA: string = 'qa';
  static readonly QUICKNOTE: string = 'quicknote';
  static readonly CORRECTIONS: string = 'corrections';
  static readonly LOCALASR: string = 'localasr';
  static readonly CREDENTIALS: string = 'credentials';

  /** Tab bar / side bar order. */
  static readonly PRIMARY: string[] = [Nav.OVERVIEW, Nav.HISTORY, Nav.STYLE, Nav.VOCAB, Nav.SETTINGS];

  static items(): NavItem[] {
    return [
      { id: Nav.OVERVIEW, label: '概览', group: '主要' },
      { id: Nav.HISTORY, label: '历史', group: '主要' },
      { id: Nav.STYLE, label: '风格', group: '主要' },
      { id: Nav.VOCAB, label: '词典', group: '主要' },
      { id: Nav.SETTINGS, label: '设置', group: '主要' }
    ];
  }

  /** Shown as a list inside 设置. */
  static tools(): NavItem[] {
    return [
      { id: Nav.CREDENTIALS, label: '凭据与密钥', group: '内容与工具' },
      { id: Nav.MARKETPLACE, label: '风格市场', group: '内容与工具' },
      { id: Nav.CORRECTIONS, label: '纠正规则', group: '内容与工具' },
      { id: Nav.TRANSLATION, label: '翻译', group: '内容与工具' },
      { id: Nav.QA, label: '问答', group: '内容与工具' },
      { id: Nav.QUICKNOTE, label: '速记', group: '内容与工具' },
      { id: Nav.LOCALASR, label: '模型设置', group: '内容与工具' }
    ];
  }

  static label(id: string): string {
    const main: NavItem[] = Nav.items();
    for (let index = 0; index < main.length; index++) {
      if (main[index].id === id) return main[index].label;
    }
    const tools: NavItem[] = Nav.tools();
    for (let index = 0; index < tools.length; index++) {
      if (tools[index].id === id) return tools[index].label;
    }
    return '概览';
  }

  /** True when a secondary page is open, so the tab bar can keep the owning tab lit. */
  static ownerOf(id: string): string {
    if (id === Nav.MARKETPLACE) return Nav.STYLE;
    if (id === Nav.CORRECTIONS) return Nav.VOCAB;
    if (id === Nav.CREDENTIALS || id === Nav.LOCALASR || id === Nav.TRANSLATION ||
      id === Nav.QA || id === Nav.QUICKNOTE) return Nav.SETTINGS;
    return id;
  }

  /** Secondary screens get a back affordance to the page that owns them. */
  static isSecondary(id: string): boolean {
    return Nav.ownerOf(id) !== id;
  }

  static go(id: string): void {
    Store.put<string>('route', id);
    Store.put<string>('detailId', '');
  }
}
