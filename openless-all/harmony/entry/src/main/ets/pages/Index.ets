import { webview } from '@kit.ArkWeb';
import { common } from '@kit.AbilityKit';
import nativeHost from 'libopenless_harmony.so';

import { HarmonyApi } from '../HarmonyApi';

@Entry
@Component
struct Index {
  private controller: webview.WebviewController = new webview.WebviewController();

  private api: HarmonyApi = new HarmonyApi();
  @State private backendReady: boolean = false;
  @State private startupError: string = '';

  aboutToAppear(): void {
    try {
      const context = this.getUIContext().getHostContext() as common.UIAbilityContext;
      console.info('[OpenLess Native] backend dataDir=' + context.filesDir + '/openless');
      this.api.configure(context, this.controller);
      const result = nativeHost.initBackend(context.filesDir + '/openless', context.cacheDir + '/openless');
      const response: Record<string, Object> = JSON.parse(result);
      if (response['ok'] !== true) {
        this.startupError = String(response['error']);
      } else {
        this.api.restoreCredentials().then(() => {
          this.backendReady = true;
          console.info('[OpenLess Native] Rust core and secure vault ready');
        }).catch((error: Error) => { this.startupError = String(error); });
      }
    } catch (error) {
      this.startupError = String(error);
    }
  }

  aboutToDisappear(): void {
    this.api.release().catch(() => {});
  }

  onPageHide(): void {
    this.api.release().catch(() => {});
  }

  build() {
    Column() {
      if (this.startupError) {
        Text('后端启动失败：' + this.startupError).padding(24)
      } else if (!this.backendReady) {
        Text('正在启动后端…').padding(24)
      } else {
      Web({ src: $rawfile('web/index.html'), controller: this.controller })
        .javaScriptProxy({ object: this.api, name: 'OpenLessHarmonyApi', methodList: ['invoke', 'request', 'takeResult'], controller: this.controller })
        .javaScriptAccess(true)
        .domStorageAccess(true)
        .width('100%')
        .height('100%')
        .onLoadIntercept((event) => {
          if (event && event.data.isMainFrame()) {
            const url = event.data.getRequestUrl();
            if (/^https?:\/\//i.test(url)) {
              this.api.openExternal(url).catch((error: Error) => { console.error('[OpenLess External] ' + String(error)); });
              return true;
            }
          }
          return false;
        })
        .onWindowNew((event) => {
          if (event) this.api.openExternal(event.targetUrl).catch((error: Error) => { console.error('[OpenLess External] ' + String(error)); });
        })
        .onPageEnd(() => {
          this.controller.runJavaScript("JSON.stringify({url:location.href,root:!!document.getElementById('root'),children:document.getElementById('root')?.childElementCount,scripts:Array.from(document.scripts).map(s=>s.src)})")
            .then((result: string) => {
              console.error('[OpenLess Page] ' + result);
            });
        })
        .onErrorReceive((event) => {
          if (event) {
            console.error('[OpenLess Load] ' + event.request.getRequestUrl() + ' ' + event.error.getErrorInfo());
          }
        })
        .onConsole((event) => {
          if (event) {
            this.api.appendLog(event.message.getMessage());
            console.info('[OpenLess Web] ' + event.message.getMessage());
          }
          return false;
        })
      }
    }
    .width('100%')
    .height('100%')
  }
}
