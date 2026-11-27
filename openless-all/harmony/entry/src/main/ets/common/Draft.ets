import { preferences } from '@kit.ArkData';
import { common } from '@kit.AbilityKit';

/**
 * 速记 draft (quick note) persistence.
 *
 * The scratchpad lives in its own local preferences file next to the dashboard
 * selection instead of the shared preference document, so the Rust core schema
 * stays untouched. One string under one key: 速记 is a scratchpad, not a document
 * store, and the history page only needs to restore the last text it saw.
 */
export class Draft {
  private static readonly STORE_NAME: string = 'home_draft';
  private static readonly KEY: string = 'text';

  private static store?: preferences.Preferences;

  /** Opens the file once and reuses it for the lifetime of the process. */
  private static async open(context: common.UIAbilityContext): Promise<preferences.Preferences> {
    const existing: preferences.Preferences | undefined = Draft.store;
    if (existing !== undefined) return existing;
    const opened: preferences.Preferences = await preferences.getPreferences(context, { name: Draft.STORE_NAME });
    Draft.store = opened;
    return opened;
  }

  static async load(context: common.UIAbilityContext): Promise<string> {
    try {
      const store: preferences.Preferences = await Draft.open(context);
      const raw: string = String(await store.get(Draft.KEY, ''));
      return raw;
    } catch (error) {
      console.warn('[OpenLess Draft] preferences unavailable, starting empty');
      return '';
    }
  }

  static async save(context: common.UIAbilityContext, text: string): Promise<void> {
    try {
      const store: preferences.Preferences = await Draft.open(context);
      await store.put(Draft.KEY, text);
      await store.flush();
    } catch (error) {
      console.warn('[OpenLess Draft] failed to save');
    }
  }

  static async clear(context: common.UIAbilityContext): Promise<void> {
    try {
      const store: preferences.Preferences = await Draft.open(context);
      await store.delete(Draft.KEY);
      await store.flush();
    } catch (error) {
      console.warn('[OpenLess Draft] failed to clear');
    }
  }
}
