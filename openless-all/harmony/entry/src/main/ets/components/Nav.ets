import { Store } from '../common/Store';

export interface NavItem {
  id: string;
  label: string;
  group: string;
}

/** Product destinations mirror iOS; presentation (dock/rail) is window-driven. */
export class Nav {
  static readonly OVERVIEW: string = 'overview';
  static readonly HISTORY: string = 'history';
  static readonly VOCAB: string = 'vocab';
  static readonly STYLE: string = 'style';
  static readonly SETTINGS: string = 'settings';
  static readonly SELECTION: string = 'selection';
  static readonly TRANSLATION: string = 'translation';
  static readonly CORRECTIONS: string = 'corrections';
  static readonly QA: string = 'qa';
  static readonly CREDENTIALS: string = 'credentials';
  static readonly LOCALASR: string = 'localasr';

  /** Stable keyboard order; the compact dock presents style as a separate group. */
  static readonly PRIMARY: string[] = [
    Nav.OVERVIEW, Nav.HISTORY, Nav.VOCAB, Nav.STYLE, Nav.SETTINGS
  ];
  static readonly DOCK_MAIN: string[] = [Nav.OVERVIEW, Nav.HISTORY, Nav.VOCAB, Nav.SETTINGS];

  static items(): NavItem[] {
    return [
      { id: Nav.OVERVIEW, label: '概览', group: '主要' },
      { id: Nav.HISTORY, label: '历史', group: '主要' },
      { id: Nav.VOCAB, label: '词典', group: '主要' },
      { id: Nav.STYLE, label: '风格', group: '主要' },
      { id: Nav.SETTINGS, label: '设置', group: '主要' }
    ];
  }

  static tools(): NavItem[] {
    return [
      { id: Nav.SELECTION, label: '选取助手', group: '内容与工具' },
      { id: Nav.TRANSLATION, label: '翻译', group: '风格' },
      { id: Nav.CORRECTIONS, label: '纠错规则', group: '词典' },
      { id: Nav.QA, label: '问答', group: '内容与工具' },
      { id: Nav.CREDENTIALS, label: '大模型配置', group: '语音与模型' },
      { id: Nav.LOCALASR, label: '端侧模型', group: '语音与模型' }
    ];
  }

  static label(id: string): string {
    const items: NavItem[] = Nav.items().concat(Nav.tools());
    for (let index = 0; index < items.length; index++) {
      if (items[index].id === id) return items[index].label;
    }
    return '概览';
  }

  static ownerOf(id: string): string {
    if (id === Nav.CORRECTIONS) return Nav.VOCAB;
    // 翻译目标 is the fourth item of the expanded styles dock, so it is a
    // destination of its own (no back bar), exactly like the iOS section list.
    if (id === Nav.TRANSLATION) return Nav.TRANSLATION;
    if (id === Nav.SELECTION || id === Nav.CREDENTIALS || id === Nav.LOCALASR || id === Nav.QA) {
      return Nav.SETTINGS;
    }
    return id;
  }

  static rootOf(id: string): string {
    let current: string = id;
    for (let guard = 0; guard < 8; guard++) {
      const owner: string = Nav.ownerOf(current);
      if (owner === current) return current;
      current = owner;
    }
    return Nav.OVERVIEW;
  }

  static known(id: string): boolean {
    const items: NavItem[] = Nav.items().concat(Nav.tools());
    for (let index = 0; index < items.length; index++) {
      if (items[index].id === id) return true;
    }
    return false;
  }

  static isSecondary(id: string): boolean {
    return Nav.ownerOf(id) !== id;
  }

  /** A custom modal blocks every navigation entry, including deep links. */
  static blocked(): boolean {
    return AppStorage.get<boolean>('overlayOpen') === true;
  }

  /** Back asks the owner to dismiss; busy editors keep the modal flag intact. */
  static dismissOverlay(): boolean {
    if (!Nav.blocked()) return false;
    Store.put<number>('overlayBackRequest', (AppStorage.get<number>('overlayBackRequest') ?? 0) + 1);
    return true;
  }

  static backTarget(id: string): string {
    if (id === Nav.CREDENTIALS) {
      const origin: string = AppStorage.get<string>('credentialsReturn') ?? Nav.SETTINGS;
      return Nav.known(origin) && origin !== Nav.CREDENTIALS ? origin : Nav.SETTINGS;
    }
    return Nav.ownerOf(id);
  }

  /** Preserve each destination's selected detail when changing tabs or layout. */
  static go(id: string): void {
    if (Nav.blocked() || !Nav.known(id)) return;
    const current: string = AppStorage.get<string>('route') ?? Nav.OVERVIEW;
    if (id === current) return;
    if (id === Nav.CREDENTIALS) Store.put<string>('credentialsReturn', current);
    Store.put<string>('navDetail-' + current, AppStorage.get<string>('detailId') ?? '');
    // Credentials is a transient setup flow, not a sticky Settings tab path.
    if (current !== Nav.CREDENTIALS) Store.put<string>('navPath-' + Nav.rootOf(current), current);
    Store.put<string>('detailId', AppStorage.get<string>('navDetail-' + id) ?? '');
    Store.put<string>('route', id);
    if (id !== Nav.CREDENTIALS) Store.put<string>('navPath-' + Nav.rootOf(id), id);
    if (id === Nav.STYLE || id === Nav.TRANSLATION) Store.put<string>('navStyleDestination', id);
  }

  static openPrimary(id: string): void {
    if (Nav.blocked()) return;
    if (id === Nav.STYLE) {
      const style: string = AppStorage.get<string>('navStyleDestination') ?? Nav.STYLE;
      Nav.go(style === Nav.TRANSLATION ? Nav.TRANSLATION : Nav.STYLE);
      return;
    }
    const saved: string = AppStorage.get<string>('navPath-' + id) ?? id;
    Nav.go(saved !== Nav.CREDENTIALS && Nav.rootOf(saved) === id ? saved : id);
  }

  /** System back and the page back affordance agree on the owning destination. */
  static back(): boolean {
    if (Nav.dismissOverlay()) return true;
    const detail: string = AppStorage.get<string>('detailId') ?? '';
    if (detail.length > 0) {
      Store.put<string>('detailId', '');
      return true;
    }
    const current: string = AppStorage.get<string>('route') ?? Nav.OVERVIEW;
    if (Nav.isSecondary(current)) {
      Nav.go(Nav.backTarget(current));
      return true;
    }
    if (current !== Nav.OVERVIEW) {
      Nav.openPrimary(Nav.OVERVIEW);
      return true;
    }
    return false;
  }
}
