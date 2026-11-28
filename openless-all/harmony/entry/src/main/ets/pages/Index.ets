import { bundleManager, common } from '@kit.AbilityKit';
import { KeyboardAvoidMode, window } from '@kit.ArkUI';
import nativeHost from 'libopenless_harmony.so';
import { Appearance } from '../common/Appearance';
import { WindowWatcher } from '../common/Breakpoint';
import { Boot } from '../common/Boot';
import { Store } from '../common/Store';
import { Palette, Radius, Space, Type, Leading } from '../common/Theme';
import { AppShell } from '../components/AppShell';
import { symbolOf } from '../components/Icons';
import { Nav } from '../components/Nav';
import { Api, Runtime } from '../data/Api';
import { SettingsSnapshot, UserPreferences } from '../data/Types';

interface BootReply {
  ok: boolean;
  error?: string;
}

interface NativeError {
  message: string;
}

/**
 * Native entry point v2.
 *
 * Owns the window size class, boots the Rust core, applies the saved appearance
 * and hosts the route table inside the adaptive shell. The system insets the
 * window (no full-screen drawing), so status bar and gesture area are handled by
 * the platform instead of hand-patched paddings.
 */
@Entry
@Component
struct Index {
  private watcher: WindowWatcher = new WindowWatcher();
  private keyboardWindow?: window.Window;
  private keyboardListener?: (height: number) => void;
  @StorageLink('route') route: string = Nav.OVERVIEW;
  @StorageLink('detailId') detailId: string = '';
  @StorageLink('bp') bp: string = 'md';
  @State private ready: boolean = false;
  @State private startupError: string = '';
  /** A new Want while the app is running arrives here, not in aboutToAppear. */
  @StorageLink('deepLinkRoute') @Watch('onDeepLink') deepLinkRoute: string = '';

  onPageShow(): void {
    Store.put<boolean>('appForeground', true);
    // The window may have been resized (split, fold, free-form) while backgrounded.
    this.watchWindow();
  }

  onPageHide(): void {
    Store.put<boolean>('appForeground', false);
  }

  aboutToAppear(): void {
    Store.put<boolean>('appForeground', true);
    // A deep link (`aa start --ps route settings`) wins over the default tab.
    const deepLink: string = Boot.takeRoute();
    Store.put<string>('route', deepLink.length > 0 ? deepLink : Nav.OVERVIEW);
    Store.put<string>('detailId', '');
    console.info('[OpenLess Boot] route=' + (deepLink.length > 0 ? deepLink : Nav.OVERVIEW));
    AppStorage.setOrCreate<string>('bp', 'md');
    AppStorage.setOrCreate<string>('dictPhase', 'idle');
    AppStorage.setOrCreate<number>('dictLevel', 0);
    AppStorage.setOrCreate<string>('dictError', '');
    AppStorage.setOrCreate<number>('backendRevision', 0);
    AppStorage.setOrCreate<number>('micLevel', 0);
    AppStorage.setOrCreate<boolean>('reduceMotion', false);
    this.watchWindow();
    this.boot();
  }

  aboutToDisappear(): void {
    Store.put<boolean>('appForeground', false);
    this.watcher.stop();
    this.stopKeyboardWatch();
    Api.bridgeOf().release();
  }

  private onDeepLink(): void {
    const route: string = this.deepLinkRoute;
    if (route.length === 0) return;
    Store.put<string>('deepLinkRoute', '');
    Nav.go(Nav.known(route) ? route : Nav.OVERVIEW);
  }

  /** Same owning-page policy for system back and the visible back affordance. */
  onBackPress(): boolean {
    // Nav requests dismissal from the modal owner; it never clears busy state.
    return Nav.back();
  }

  /** Main-window changes are synchronous with the platform's rotation animation.
   * Do not add a delayed whole-page fade/spring or remount AppShell on resize.
   * https://developer.huawei.com/consumer/cn/doc/doccenter-capabilities/arkts-rotation-transition-animation
   */
  private watchWindow(): void {
    const mainWindow = Boot.mainWindow;
    if (mainWindow === undefined) return;
    // Floating input/toolbars need native viewport compression, not OFFSET's
    // whole-page pan, which can carry their close controls out of the viewport.
    // https://developer.huawei.com/consumer/cn/doc/doccenter-dev-faq/faqs-arkui-1073
    try {
      mainWindow.getUIContext().setKeyboardAvoidMode(KeyboardAvoidMode.RESIZE);
    } catch (error) {
      console.warn('[OpenLess Window] keyboard resize avoidance unavailable');
    }
    this.watchKeyboard(mainWindow);
    this.watcher.start(mainWindow, (next: string, shortWindow: boolean, portrait: boolean) => {
      Store.put<string>('bp', next);
      Store.put<boolean>('shortWindow', shortWindow);
      Store.put<boolean>('portrait', portrait);
      Store.put<boolean>('phoneLayout', portrait);
      console.info('[OpenLess Window] layout=' + next + ', portrait=' + portrait + ', short=' + shortWindow);
    });
  }

  /** Observe the owning main window; never guess keyboard state from focus/resize.
   * https://developer.huawei.com/consumer/cn/doc/doccenter-dev-faq/faqs-arkui-1073
   * Leave native avoidance enabled: this only removes the dock's hit targets.
   */
  private watchKeyboard(target: window.Window): void {
    if (this.keyboardWindow === target) return;
    this.stopKeyboardWatch();
    const listener = (height: number): void => {
      if (this.keyboardWindow !== target || this.keyboardListener !== listener) return;
      Store.put<boolean>('keyboardVisible', height > 0);
    };
    try {
      this.keyboardWindow = target;
      this.keyboardListener = listener;
      target.on('keyboardHeightChange', listener);
    } catch (error) {
      this.keyboardWindow = undefined;
      this.keyboardListener = undefined;
      console.warn('[OpenLess Window] keyboard listener unavailable');
    }
  }

  private stopKeyboardWatch(): void {
    const target = this.keyboardWindow;
    const listener = this.keyboardListener;
    this.keyboardWindow = undefined;
    this.keyboardListener = undefined;
    if (target !== undefined && listener !== undefined) {
      try {
        target.off('keyboardHeightChange', listener);
      } catch (error) {
        console.warn('[OpenLess Window] keyboard listener detach failed');
      }
    }
    Store.put<boolean>('keyboardVisible', false);
  }

  private async boot(): Promise<void> {
    try {
      const context = this.getUIContext().getHostContext() as common.UIAbilityContext;
      Runtime.attach(context);
      const wire: string = nativeHost.initBackend(context.filesDir + '/openless', context.cacheDir + '/openless');
      const reply: BootReply = JSON.parse(wire) as BootReply;
      if (!reply.ok) {
        this.startupError = reply.error ?? '后端启动失败';
        return;
      }
      try {
        const info = bundleManager.getBundleInfoForSelfSync(
          bundleManager.BundleFlag.GET_BUNDLE_INFO_DEFAULT);
        Store.put<string>('appVersion', info.versionName);
      } catch (error) {
        console.warn('[OpenLess Boot] version unavailable');
      }
      await Api.bridgeOf().restoreCredentials();
      await this.applySavedAppearance(context);
      this.ready = true;
      console.info('[OpenLess Native] core ready');
    } catch (error) {
      this.startupError = (error as NativeError).message;
    }
  }

  /** The theme preference is applied before the first frame of the shell. */
  private async applySavedAppearance(context: common.UIAbilityContext): Promise<void> {
    try {
      const snapshot: SettingsSnapshot = await Api.settings();
      const prefs: UserPreferences = snapshot.preferences;
      Appearance.apply(context, prefs.themeMode ?? 'system');
    } catch (error) {
      console.warn('[OpenLess Boot] appearance preference unavailable');
    }
  }

  build() {
    Column() {
      if (this.startupError.length > 0) {
        this.failure()
      } else if (!this.ready) {
        this.loading()
      } else {
        AppShell()
      }
    }
    .width('100%')
    .height('100%')
    .backgroundColor(Palette.bg())
  }

  @Builder
  loading() {
    Column({ space: Space.sm }) {
      LoadingProgress()
        .width(40)
        .height(40)
        .color(Palette.accent())
      Text('正在启动本地后端')
        .fontSize(Type.label)
        .lineHeight(Leading.label)
        .fontColor(Palette.text3())
    }
    .width('100%')
    .height('100%')
    .justifyContent(FlexAlign.Center)
    .backgroundColor(Palette.bg())
  }

  @Builder
  failure() {
    Column({ space: Space.sm }) {
      SymbolGlyph(symbolOf('close'))
        .fontSize(28)
        .fontColor([Palette.danger()])
      Text('后端启动失败')
        .fontSize(Type.section)
        .lineHeight(Leading.section)
        .fontWeight(FontWeight.Medium)
        .fontColor(Palette.text1())
      Text(this.startupError)
        .fontSize(Type.label)
        .lineHeight(Leading.label)
        .fontColor(Palette.text2())
        .padding(Space.sm)
        .backgroundColor(Palette.dangerSoft())
        .borderRadius(Radius.card)
        .textAlign(TextAlign.Center)
      Button('重试', { type: ButtonType.Capsule })
        .height(44)
        .fontSize(Type.body)
        .backgroundColor(Palette.accent())
        .fontColor(Palette.onAccent())
        .onClick(() => {
          this.startupError = '';
          this.boot();
        })
    }
    .width('100%')
    .height('100%')
    .justifyContent(FlexAlign.Center)
    .padding(Space.xl)
    .backgroundColor(Palette.bg())
  }
}
