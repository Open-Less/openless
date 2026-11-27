import { UIAbility, Want, abilityAccessCtrl } from '@kit.AbilityKit';
import { window } from '@kit.ArkUI';

export default class EntryAbility extends UIAbility {
  private clipboardRequested: boolean = false;

  onCreate(want?: Want): void {
    this.clipboardRequested = want?.parameters?.requestClipboardPermission === true;
  }

  onNewWant(want?: Want): void {
    this.clipboardRequested = want?.parameters?.requestClipboardPermission === true;
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
    windowStage.loadContent('pages/Index', () => {
      if (this.clipboardRequested) { this.clipboardRequested = false; this.requestClipboard(); }
    });
  }
}
