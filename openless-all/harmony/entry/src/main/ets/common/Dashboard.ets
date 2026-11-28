import { preferences } from '@kit.ArkData';
import { common } from '@kit.AbilityKit';

export interface DashboardCard {
  id: string;
  title: string;
  hint: string;
}

/**
 * Home dashboard composition.
 *
 * The overview is a card grid the user composes: every info card can be hidden or
 * shown from 「自定义主页」. The choice lives in a local preferences file (not in
 * the shared preference document) so the Rust core schema stays untouched.
 */
export class Dashboard {
  static readonly TODAY: string = 'today';
  static readonly PERIOD: string = 'period';
  static readonly HEATMAP: string = 'heatmap';
  static readonly RECENT: string = 'recent';
  static readonly CONFIG: string = 'config';
  static readonly SHORTCUTS: string = 'shortcuts';

  /** Registry, also the display order of the grid. */
  static readonly CARDS: DashboardCard[] = [
    { id: Dashboard.TODAY, title: '今日数据', hint: '字数、时长、段数与累计记录' },
    { id: Dashboard.PERIOD, title: '最近趋势', hint: '近 7 / 30 天的使用趋势' },
    { id: Dashboard.HEATMAP, title: '活动热力图', hint: '近 12 周每天的听写量' },
    { id: Dashboard.RECENT, title: '最近记录', hint: '最新三条听写，可跳转历史' },
    { id: Dashboard.CONFIG, title: '当前配置', hint: '引擎、模型、麦克风与网络状态' },
    { id: Dashboard.SHORTCUTS, title: '快捷入口', hint: '翻译、问答、速记与风格市场' }
  ];

  private static store?: preferences.Preferences;

  static defaults(): string[] {
    const ids: string[] = [];
    for (let index = 0; index < Dashboard.CARDS.length; index++) {
      ids.push(Dashboard.CARDS[index].id);
    }
    return ids;
  }

  static titleOf(id: string): string {
    for (let index = 0; index < Dashboard.CARDS.length; index++) {
      if (Dashboard.CARDS[index].id === id) return Dashboard.CARDS[index].title;
    }
    return id;
  }

  static hintOf(id: string): string {
    for (let index = 0; index < Dashboard.CARDS.length; index++) {
      if (Dashboard.CARDS[index].id === id) return Dashboard.CARDS[index].hint;
    }
    return '';
  }

  /** Keeps the registry order and drops unknown / duplicated ids. */
  static normalize(ids: string[]): string[] {
    const result: string[] = [];
    for (let index = 0; index < Dashboard.CARDS.length; index++) {
      const id: string = Dashboard.CARDS[index].id;
      if (ids.indexOf(id) >= 0) result.push(id);
    }
    return result;
  }

  static async load(context: common.UIAbilityContext): Promise<string[]> {
    try {
      if (Dashboard.store === undefined) {
        Dashboard.store = await preferences.getPreferences(context, { name: 'home_dashboard' });
      }
      const raw: string = String(await Dashboard.store.get('cards', ''));
      if (raw.length === 0) return Dashboard.defaults();
      const parsed: string[] = JSON.parse(raw) as string[];
      const normalized: string[] = Dashboard.normalize(parsed);
      return normalized.length > 0 ? normalized : Dashboard.defaults();
    } catch (error) {
      console.warn('[OpenLess Dashboard] preferences unavailable, using defaults');
      return Dashboard.defaults();
    }
  }

  static async save(context: common.UIAbilityContext, ids: string[]): Promise<void> {
    try {
      if (Dashboard.store === undefined) {
        Dashboard.store = await preferences.getPreferences(context, { name: 'home_dashboard' });
      }
      await Dashboard.store.put('cards', JSON.stringify(Dashboard.normalize(ids)));
      await Dashboard.store.flush();
    } catch (error) {
      console.warn('[OpenLess Dashboard] failed to save card selection');
    }
  }
}
