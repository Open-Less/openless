import { window } from '@kit.ArkUI';

/** Huawei recommended window-width boundaries (xs folds into compact sm).
 * https://developer.huawei.com/consumer/cn/doc/best-practices-V5/bpta-multi-device-bp-practice-V5
 * No device-type/orientation lock: split and floating windows share this path.
 */
export class Breakpoints {
  static readonly SM: string = 'sm';
  static readonly MD: string = 'md';
  static readonly LG: string = 'lg';
  static readonly XL: string = 'xl';

  static isCompact(bp: string): boolean {
    return bp === Breakpoints.SM;
  }

  static isWide(bp: string): boolean {
    return bp === Breakpoints.LG || bp === Breakpoints.XL;
  }

  static isExtraWide(bp: string): boolean {
    return bp === Breakpoints.XL;
  }

  static of(widthVp: number): string {
    if (widthVp < 600) return Breakpoints.SM;
    if (widthVp < 840) return Breakpoints.MD;
    if (widthVp < 1440) return Breakpoints.LG;
    return Breakpoints.XL;
  }
}

export class WindowWatcher {
  private win?: window.Window;
  private callback?: (bp: string, shortWindow: boolean, portrait: boolean) => void;
  private current: string = Breakpoints.SM;
  private shortWindow: boolean = false;
  private portrait: boolean = true;
  private initialized: boolean = false;
  private generation: number = 0;
  private sizeListener?: (size: window.Size) => void;

  value(): string {
    return this.current;
  }

  start(target: window.Window, onChange: (bp: string, shortWindow: boolean, portrait: boolean) => void): void {
    this.stop();
    this.win = target;
    this.callback = onChange;
    const generation: number = this.generation;
    this.sizeListener = (size: window.Size): void => {
      // An already queued event from a destroyed/replaced window is obsolete.
      if (generation !== this.generation || this.win !== target) return;
      this.apply(size.width, size.height);
    };
    try {
      const properties = target.getWindowProperties();
      this.apply(properties.windowRect.width, properties.windowRect.height);
      target.on('windowSizeChange', this.sizeListener);
    } catch (error) {
      console.error('[OpenLess Window] watcher failed: ' + JSON.stringify(error));
    }
  }

  stop(): void {
    this.generation++;
    if (this.win !== undefined && this.sizeListener !== undefined) {
      try {
        this.win.off('windowSizeChange', this.sizeListener);
      } catch (error) {
        console.warn('[OpenLess Window] detach failed');
      }
    }
    this.win = undefined;
    this.callback = undefined;
    this.sizeListener = undefined;
    this.initialized = false;
  }

  private apply(widthPx: number, heightPx: number): void {
    if (this.win === undefined || widthPx <= 0 || heightPx <= 0) return;
    // Bind conversion to this window's UIContext, not the default display density.
    // https://developer.huawei.com/consumer/cn/doc/harmonyos-faqs/faqs-arkui-753
    const context = this.win.getUIContext();
    const widthVp: number = context.px2vp(widthPx);
    const heightVp: number = context.px2vp(heightPx);
    const next: string = Breakpoints.of(widthVp);
    const short: boolean = heightVp < 480;
    // Portrait keeps the phone presentation on tablets (iOS demo layout).
    const portrait: boolean = heightVp >= widthVp;
    if (this.initialized && next === this.current && short === this.shortWindow &&
      portrait === this.portrait) return;
    this.initialized = true;
    this.current = next;
    this.shortWindow = short;
    this.portrait = portrait;
    if (this.callback !== undefined) this.callback(next, short, portrait);
  }
}
