/** Navigation model shared by the shell, the tab bar and the routes. */
import { Store } from '../common/Store';

export interface NavItem {
  id: string;
  label: string;
  group: string;
}

export class Nav {
  static readonly OVERVIEW: string = 'overview';
  static readonly HISTORY: string = 'history';
  static readonly VOCAB: string = 'vocab';
  static readonly CORRECTIONS: string = 'corrections';
  static readonly STYLE: string = 'style';
  static readonly MARKETPLACE: string = 'marketplace';
  static readonly TRANSLATION: string = 'translation';
  static readonly QA: string = 'qa';
  static readonly QUICKNOTE: string = 'quicknote';
  static readonly LOCALASR: string = 'localasr';
  static readonly SETTINGS: string = 'settings';
  static readonly MORE: string = 'more';

  /** Compact layouts keep five primary destinations and push the rest into 更多. */
  static readonly PRIMARY: string[] = [Nav.OVERVIEW, Nav.HISTORY, Nav.VOCAB, Nav.STYLE, Nav.SETTINGS];

  static items(): NavItem[] {
    const list: NavItem[] = [
      { id: Nav.OVERVIEW, label: '概览', group: '写作' },
      { id: Nav.HISTORY, label: '历史', group: '写作' },
      { id: Nav.VOCAB, label: '词典', group: '写作' },
      { id: Nav.CORRECTIONS, label: '纠正规则', group: '写作' },
      { id: Nav.STYLE, label: '风格', group: '写作' },
      { id: Nav.MARKETPLACE, label: '风格市场', group: '写作' },
      { id: Nav.TRANSLATION, label: '翻译', group: '工具' },
      { id: Nav.QA, label: '问答', group: '工具' },
      { id: Nav.QUICKNOTE, label: '速记', group: '工具' },
      { id: Nav.LOCALASR, label: '模型设置', group: '工具' },
      { id: Nav.SETTINGS, label: '设置', group: '系统' }
    ];
    return list;
  }

  static label(id: string): string {
    const list: NavItem[] = Nav.items();
    for (let index = 0; index < list.length; index++) {
      if (list[index].id === id) return list[index].label;
    }
    return '概览';
  }

  static isPrimary(id: string): boolean {
    return Nav.PRIMARY.indexOf(id) >= 0;
  }

  static go(id: string): void {
    console.info('[OpenLess Nav] ' + id);
    Store.put<string>('route', id);
    Store.put<string>('detailId', '');
  }
}
