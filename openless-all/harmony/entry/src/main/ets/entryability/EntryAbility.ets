import { UIAbility, Want, abilityAccessCtrl } from '@kit.AbilityKit';
import { window } from '@kit.ArkUI';
import { Boot } from '../common/Boot';
import { Store } from '../common/Store';

export default class EntryAbility extends UIAbility {
  private clipboardRequested: boolean = false;
  private pageLoaded: boolean = false;

  /**
   * `aa start --ps route settings` (or any future notification/deep link) opens
   * the app on that destination. The value is only a hint: `Index` falls back to
   * the overview when the route is unknown.
   */
  private static routeOf(want?: Want): string {
    const value = want?.parameters?.route;
    return typeof value === 'string' ? value : '';
  }

  private initialRoute: string = '';

  onCreate(want?: Want): void {
    this.clipboardRequested = want?.parameters?.requestClipboardPermission === true;
    this.initialRoute = EntryAbility.routeOf(want);
    Boot.route = this.initialRoute;
  }

  onNewWant(want?: Want): void {
    this.clipboardRequested = want?.parameters?.requestClipboardPermission === true;
    const route = EntryAbility.routeOf(want);
    if (route.length > 0) Store.put<string>('deepLinkRoute', route);
  }

  onWindowStageDestroy(): void {
    Boot.mainWindow = undefined;
    if (this.pageLoaded) Store.put<boolean>('appForeground', false);
    this.pageLoaded = false;
    this.initialRoute = '';
  }

  onForeground(): void {
    if (this.pageLoaded) Store.put<boolean>('appForeground', true);
    if (this.clipboardRequested) this.requestClipboard();
  }

  onBackground(): void {
    if (this.pageLoaded) Store.put<boolean>('appForeground', false);
  }

  private requestClipboard(): void {
    this.clipboardRequested = false;
    if (!this.context.resourceManager.getBooleanByName('ime_clipboard_auto_read')) return;
    abilityAccessCtrl.createAtManager().requestPermissionsFromUser(this.context, ['ohos.permission.READ_PASTEBOARD'])
      .then((result) => { console.info('[OpenLess] clipboard permission result=' + result.authResults.join(',')); })
      .catch(() => { console.warn('[OpenLess] clipboard permission request unavailable'); });
  }

  onWindowStageCreate(windowStage: window.WindowStage): void {
    // Bind before loadContent so Index observes the owning main window, never
    // an overlay returned by getLastWindow. Huawei rotation animation guide:
    // https://developer.huawei.com/consumer/cn/doc/doccenter-capabilities/arkts-rotation-transition-animation
    try {
      Boot.mainWindow = windowStage.getMainWindowSync();
    } catch (error) {
      Boot.mainWindow = undefined;
      console.error('[OpenLess Window] main window unavailable: ' + JSON.stringify(error));
    }
    Boot.route = this.initialRoute;
    windowStage.loadContent('pages/Index', () => {
      this.pageLoaded = true;
      if (this.clipboardRequested) { this.clipboardRequested = false; this.requestClipboard(); }
    });
  }
}
