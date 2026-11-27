import { InputMethodExtensionAbility, inputMethodEngine } from '@kit.IMEKit';
import { display } from '@kit.ArkUI';
import { panelLayout } from './PanelLayout';
import { imeStorage, inputSession } from './InputSession';
import { initializeRime } from './RimeClient';

export default class OpenLessInputMethod extends InputMethodExtensionAbility {
  private engine: inputMethodEngine.InputMethodAbility = inputMethodEngine.getInputMethodAbility();
  private delegate: inputMethodEngine.KeyboardDelegate = inputMethodEngine.getKeyboardDelegate();
  private onText = (text: string): void => { inputSession.observeText(text); };
  private onSelection = (_oldBegin: number, _oldEnd: number, begin: number, end: number): void => {
    inputSession.observeSelection(begin, end);
  };
  private panel?: inputMethodEngine.Panel;
  private ready?: Promise<void>;
  private active: boolean = false;
  private visible: boolean = false;
  private onDisplayChange = (): void => {
    if (this.active && this.panel) panelLayout.refresh();
  };

  private onStart = (keyboard: inputMethodEngine.KeyboardController, client: inputMethodEngine.InputClient): void => {
    inputSession.attach(this.context, keyboard, client);
    console.info('[OpenLess IME] inputStart');
  };
  private onStop = (): void => {
    inputSession.detach();
    this.visible = false;
    this.panel?.hide().catch(() => {});
    console.info('[OpenLess IME] inputStop');
  };
  private onShow = (): void => {
    this.visible = true;
    this.ensurePanel().then(() => {
      if (this.active && this.visible) {
        return panelLayout.refresh().then(() => {
          if (this.active && this.visible) return this.panel?.show();
          return undefined;
        });
      }
      return undefined;
    }).catch(() => { imeStorage.setOrCreate('status', '键盘加载失败'); });
  };
  private onHide = (): void => {
    inputSession.cancelVoice();
    this.visible = false;
    this.panel?.hide().catch(() => {});
  };

  onCreate(): void {
    this.active = true;
    this.delegate.on('textChange', this.onText);
    this.delegate.on('selectionChange', this.onSelection);
    this.engine.on('inputStart', this.onStart);
    this.engine.on('inputStop', this.onStop);
    this.engine.on('keyboardShow', this.onShow);
    this.engine.on('keyboardHide', this.onHide);
    display.on('change', this.onDisplayChange);
    // Prepare dictionaries in parallel with panel creation and editor attributes.
    // This creates no session and never enables learning or network queries.
    initializeRime(this.context.filesDir, this.context.resourceManager, 'rime-user', this.context.cacheDir).catch(() => {
      console.warn('[OpenLess IME] dictionary prewarm failed; retry on input');
    });
    console.info('[OpenLess IME] created');
  }

  private ensurePanel(): Promise<void> {
    if (this.ready) return this.ready;
    this.ready = this.createPanel().catch(() => {
      this.ready = undefined;
      console.error('[OpenLess IME] panel creation failed');
      throw new Error('panel creation failed');
    });
    return this.ready;
  }

  private async createPanel(): Promise<void> {
    const panel = await this.engine.createPanel(this.context, {
      type: inputMethodEngine.PanelType.SOFT_KEYBOARD,
      flag: inputMethodEngine.PanelFlag.FLG_FIXED
    });
    if (!this.active) {
      await this.engine.destroyPanel(panel);
      return;
    }
    this.panel = panel;
    try {
      await panelLayout.attach(panel, this.context);
      await panel.setUiContent('pages/InputMethodPanel', imeStorage);
      console.info('[OpenLess IME] panel ready');
    } catch (_) {
      this.panel = undefined;
      panelLayout.detach();
      await this.engine.destroyPanel(panel);
      throw new Error('panel content failed');
    }
  }

  onDestroy(): void {
    this.active = false;
    this.delegate.off('textChange', this.onText);
    this.delegate.off('selectionChange', this.onSelection);
    this.visible = false;
    this.engine.off('inputStart', this.onStart);
    this.engine.off('inputStop', this.onStop);
    this.engine.off('keyboardShow', this.onShow);
    this.engine.off('keyboardHide', this.onHide);
    display.off('change', this.onDisplayChange);
    inputSession.detach();
    const panel = this.panel;
    this.panel = undefined;
    panelLayout.detach();
    if (panel) this.engine.destroyPanel(panel).catch(() => {});
    console.info('[OpenLess IME] destroyed');
  }
}
