import { common } from '@kit.AbilityKit';
import { window } from '@kit.ArkUI';
import { BusinessError } from '@kit.BasicServicesKit';
import nativeHost from 'libopenless_harmony.so';
import { WindowWatcher } from '../common/Breakpoint';
import { C, R, S, T } from '../common/Tokens';
import { AppShell } from '../components/AppShell';
import { Nav } from '../components/Nav';
import { Boot } from '../common/Boot';
import { Store } from '../common/Store';
import { Api, Runtime } from '../data/Api';

interface BootReply {
  ok: boolean;
  error?: string;
}

interface NativeError {
  message: string;
}

/**
 * Native entry point.
 *
 * The ArkWeb shell is gone: this page owns the window size class, boots the Rust
 * core, and hosts the route table inside the adaptive `AppShell`. Every page reads
 * the same core over the same bridge, so phone, tablet and 2-in-1 run one UI.
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

  /** Size class from the window rectangle: covers rotation, split and free-form. */
  private watchWindow(): void {
    const pinned: string = AppStorage.get<string>('bpOverride') ?? '';
    if (pinned.length > 0) {
      Store.put<string>('bp', pinned);
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
      await Api.bridgeOf().restoreCredentials();
      this.ready = true;
      console.info('[OpenLess Native] core ready');
    } catch (error) {
      this.startupError = (error as NativeError).message;
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
    .backgroundColor(C.bg())
  }

  @Builder
  loading() {
    Column({ space: S.md }) {
      LoadingProgress()
        .width(48)
        .height(48)
        .color(C.accent())
      Text('正在启动本地后端…')
        .fontSize(T.body)
        .fontColor(C.textSecondary())
    }
    .width('100%')
    .height('100%')
    .justifyContent(FlexAlign.Center)
  }

  @Builder
  failure() {
    Column({ space: S.md }) {
      Text('后端启动失败')
        .fontSize(T.headline)
        .fontWeight(FontWeight.Medium)
        .fontColor(C.textPrimary())
      Text(this.startupError)
        .fontSize(T.footnote)
        .fontColor(C.danger())
        .padding(S.md)
        .backgroundColor(C.dangerSoft())
        .borderRadius(R.md)
        .textAlign(TextAlign.Center)
      Button('重试')
        .backgroundColor(C.accent())
        .fontColor(C.textOnAccent())
        .borderRadius(R.pill)
        .onClick(() => {
          this.startupError = '';
          this.boot();
        })
    }
    .width('100%')
    .height('100%')
    .justifyContent(FlexAlign.Center)
    .padding(S.xl)
  }

}
