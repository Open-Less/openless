import { UIAbility, Want, abilityAccessCtrl } from '@kit.AbilityKit';
import { window } from '@kit.ArkUI';
import { Boot } from '../common/Boot';
import { Store } from '../common/Store';

export default class EntryAbility extends UIAbility {
  private clipboardRequested: boolean = false;

  /**
   * `aa start --ps route settings` (or any future notification/deep link) opens
   * the app on that destination. The value is only a hint: `Index` falls back to
   * the overview when the route is unknown.
   */
  private static routeOf(want?: Want): string {
    const value = want?.parameters?.route;
    return typeof value === 'string' ? value : '';
  }

  /**
   * `aa start --ps layout sm|md|lg` pins the size class. One UI ships for phone,
   * tablet and 2-in-1, and QA can then verify all three layouts on a single
   * device instead of rotating, splitting or resizing a window by hand.
   */
  private static layoutOf(want?: Want): string {
    const value = want?.parameters?.layout;
    if (typeof value !== 'string') return '';
    if (value === 'sm' || value === 'md' || value === 'lg') return value;
    return '';
  }

  private initialRoute: string = '';

  onCreate(want?: Want): void {
    this.clipboardRequested = want?.parameters?.requestClipboardPermission === true;
    this.initialRoute = EntryAbility.routeOf(want);
    Boot.route = this.initialRoute;
    Boot.layout = EntryAbility.layoutOf(want);
  }

  onNewWant(want?: Want): void {
    this.clipboardRequested = want?.parameters?.requestClipboardPermission === true;
    const route = EntryAbility.routeOf(want);
    if (route.length > 0) Store.put<string>('deepLinkRoute', route);
    const layout = EntryAbility.layoutOf(want);
    if (layout.length > 0) Store.put<string>('bpOverride', layout);
  }

  onWindowStageDestroy(): void {
    this.initialRoute = '';
  }

  onForeground(): void {
    if (this.clipboardRequested) this.requestClipboard();
  }

  private requestClipboard(): void {
    this.clipboardRequested = false;
    if (!this.context.resourceManager.getBooleanByName('ime_clipboard_auto_read')) return;
    abilityAccessCtrl.createAtManager().requestPermissionsFromUser(this.context, ['ohos.permission.READ_PASTEBOARD'])
      .then((result) => { console.info('[OpenLess] clipboard permission result=' + result.authResults.join(',')); })
      .catch(() => { console.warn('[OpenLess] clipboard permission request unavailable'); });
  }

  onWindowStageCreate(windowStage: window.WindowStage): void {
    // `Boot.layout` was recorded in onCreate; keep the window-stage path minimal.
    Boot.route = this.initialRoute;
    windowStage.loadContent('pages/Index', () => {
      if (this.clipboardRequested) { this.clipboardRequested = false; this.requestClipboard(); }
    });
  }
}
