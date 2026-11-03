import { Store } from '../common/Store';

/**
 * Navigation model v2.
 *
 * Four destinations share the bottom capsule dock / side rail — 主页 / 历史记录 /
 * 风格市场 / 设置. The merged 风格 page holds both the local style packs and the
 * marketplace, so the dock label stays 风格市场.
 *
 * Everything else is a secondary page listed inside 设置: 选取助手, 翻译, 词典,
 * 纠错规则, 问答, 凭据与密钥, 模型设置. 速记 lives inside 历史记录.
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
  static readonly SETTINGS: string = 'settings';

  /** Secondary destinations, reachable from 设置. */
  static readonly SELECTION: string = 'selection';
  static readonly TRANSLATION: string = 'translation';
  static readonly VOCAB: string = 'vocab';
  static readonly CORRECTIONS: string = 'corrections';
  static readonly QA: string = 'qa';
  static readonly CREDENTIALS: string = 'credentials';
  static readonly LOCALASR: string = 'localasr';

  /** Capsule dock / side rail order. */
  static readonly PRIMARY: string[] = [
    Nav.OVERVIEW,
    Nav.HISTORY,
    Nav.STYLE,
    Nav.SETTINGS
  ];

  static items(): NavItem[] {
    return [
      { id: Nav.OVERVIEW, label: '主页', group: '主要' },
      { id: Nav.HISTORY, label: '历史记录', group: '主要' },
      { id: Nav.STYLE, label: '风格市场', group: '主要' },
      { id: Nav.SETTINGS, label: '设置', group: '主要' }
    ];
  }

  /** Shown as the tool list inside 设置. */
  static tools(): NavItem[] {
    return [
      { id: Nav.SELECTION, label: '选取助手', group: '内容与工具' },
      { id: Nav.TRANSLATION, label: '翻译', group: '内容与工具' },
      { id: Nav.VOCAB, label: '词典', group: '内容与工具' },
      { id: Nav.CORRECTIONS, label: '纠错规则', group: '内容与工具' },
      { id: Nav.QA, label: '问答', group: '内容与工具' },
      { id: Nav.CREDENTIALS, label: '凭据与密钥', group: '内容与工具' },
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
    return '主页';
  }

  /** Immediate parent, used by the back affordance of a secondary page. */
  static ownerOf(id: string): string {
    if (id === Nav.CORRECTIONS) return Nav.VOCAB;
    if (id === Nav.SELECTION || id === Nav.TRANSLATION || id === Nav.VOCAB ||
      id === Nav.CREDENTIALS || id === Nav.LOCALASR || id === Nav.QA) {
      return Nav.SETTINGS;
    }
    return id;
  }

  /** Top-level destination that should stay lit in the dock / rail. */
  static rootOf(id: string): string {
    let current: string = id;
    for (let guard = 0; guard < 8; guard++) {
      const owner: string = Nav.ownerOf(current);
      if (owner === current) return current;
      current = owner;
    }
    return Nav.OVERVIEW;
  }

  static isSecondary(id: string): boolean {
    return Nav.ownerOf(id) !== id;
  }

  static go(id: string): void {
    Store.put<string>('route', id);
    Store.put<string>('detailId', '');
  }
}
