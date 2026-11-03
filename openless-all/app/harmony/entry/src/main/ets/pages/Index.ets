import { bundleManager, common } from '@kit.AbilityKit';
import { window } from '@kit.ArkUI';
import { BusinessError } from '@kit.BasicServicesKit';
import nativeHost from 'libopenless_harmony.so';
import { Appearance } from '../common/Appearance';
import { WindowWatcher } from '../common/Breakpoint';
import { Boot } from '../common/Boot';
import { Store } from '../common/Store';
import { Palette, PHONE_FIRST, Radius, Space, Type, Leading } from '../common/Theme';
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
  @StorageLink('route') route: string = Nav.OVERVIEW;
  @StorageLink('detailId') detailId: string = '';
  @StorageLink('bp') bp: string = 'md';
  @State private ready: boolean = false;
  @State private startupError: string = '';

  aboutToAppear(): void {
    // A deep link (`aa start --ps route settings`) wins over the default tab.
    const deepLink: string = Boot.takeRoute();
    const pinnedLayout: string = Boot.takeLayout();
    if (pinnedLayout.length > 0) Store.put<string>('bpOverride', pinnedLayout);
    Store.put<string>('route', deepLink.length > 0 ? deepLink : Nav.OVERVIEW);
    Store.put<string>('detailId', '');
    console.info('[OpenLess Boot] route=' + (deepLink.length > 0 ? deepLink : Nav.OVERVIEW));
    AppStorage.setOrCreate<string>('bp', 'md');
    AppStorage.setOrCreate<string>('dictPhase', 'idle');
    AppStorage.setOrCreate<number>('dictLevel', 0);
    AppStorage.setOrCreate<string>('dictError', '');
    AppStorage.setOrCreate<number>('backendRevision', 0);
    AppStorage.setOrCreate<number>('micLevel', 0);
    this.watchWindow();
    this.boot();
  }

  aboutToDisappear(): void {
    this.watcher.stop();
    Api.bridgeOf().release();
  }

  /** Android-style back: leave a detail first, then return to the first tab. */
  onBackPress(): boolean {
    if (this.detailId.length > 0) {
      Store.put<string>('detailId', '');
      return true;
    }
    if (this.route !== Nav.OVERVIEW) {
      Nav.go(Nav.OVERVIEW);
      return true;
    }
    return false;
  }

  /** Size class from the window rectangle: covers rotation, split and free-form resizing. */
  private watchWindow(): void {
    const pinned: string = AppStorage.get<string>('bpOverride') ?? '';
    if (pinned.length > 0) {
      Store.put<string>('bp', PHONE_FIRST ? 'sm' : pinned);
      console.info('[OpenLess Window] pinned size class ' + pinned);
      return;
    }
    try {
      const context = this.getUIContext().getHostContext() as common.UIAbilityContext;
      window.getLastWindow(context).then((win: window.Window) => {
        this.watcher.start(win, (next: string) => {
          AppStorage.setOrCreate<string>('bp', next);
        });
        AppStorage.setOrCreate<string>('bp', this.watcher.value());
        console.info('[OpenLess Window] size class ' + this.watcher.value());
      }).catch((error: BusinessError) => {
        console.error('[OpenLess Window] ' + JSON.stringify(error));
      });
    } catch (error) {
      console.error('[OpenLess Window] ' + JSON.stringify(error));
    }
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
