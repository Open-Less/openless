import { display, window } from '@kit.ArkUI';

/**
 * One-development-multi-deployment breakpoints.
 *
 * The HarmonyOS blueprint splits layouts at 600vp and 840vp:
 *   sm  phone portrait                        -> bottom tab bar, one column
 *   md  phone landscape / small tablet        -> icon rail, one wide column
 *   lg  tablet / 2-in-1                       -> labelled sidebar, list + detail
 *
 * The size class comes from the window's own rectangle instead of `mediaquery`:
 * a free-form 2-in-1 window is resized by the user, and `windowSizeChange` is the
 * only signal that reports those sizes reliably (media query listeners report the
 * whole-screen size and their range syntax parses differently across releases).
 * Rotation, split-screen and free-form resizing all arrive as size changes, so one
 * code path covers phone, tablet and PC.
 */
export class Breakpoints {
  static readonly SM: string = 'sm';
  static readonly MD: string = 'md';
  static readonly LG: string = 'lg';

  static isCompact(bp: string): boolean {
    return bp === Breakpoints.SM;
  }

  static isWide(bp: string): boolean {
    return bp === Breakpoints.LG;
  }

  /** Window width in vp to a size class. */
  static of(widthVp: number): string {
    if (widthVp < 600) return Breakpoints.SM;
    if (widthVp < 840) return Breakpoints.MD;
    return Breakpoints.LG;
  }
}

export class WindowWatcher {
  private win?: window.Window;
  private callback?: (bp: string) => void;
  private current: string = Breakpoints.MD;
  private density: number = 2;

  value(): string {
    return this.current;
  }

  /**
   * Subscribes to the window size; `onChange` fires only when the size class
   * changes, so a resize inside one class does not re-render the whole tree.
   */
  start(target: window.Window, onChange: (bp: string) => void): void {
    this.win = target;
    this.callback = onChange;
    this.density = WindowWatcher.readDensity();
    try {
      const properties = target.getWindowProperties();
      this.apply(properties.windowRect.width);
      target.on('windowSizeChange', this.sizeListener);
    } catch (error) {
      console.error('[OpenLess Window] watcher failed: ' + JSON.stringify(error));
    }
  }

  stop(): void {
    if (this.win !== undefined) {
      try {
        this.win.off('windowSizeChange', this.sizeListener);
      } catch (error) {
        console.warn('[OpenLess Window] detach failed');
      }
    }
    this.win = undefined;
    this.callback = undefined;
  }

  private static readDensity(): number {
    try {
      const info: display.Display = display.getDefaultDisplaySync();
      if (info.densityPixels > 0) return info.densityPixels;
    } catch (error) {
      console.warn('[OpenLess Window] density unavailable, assuming 2x');
    }
    return 2;
  }

  private sizeListener = (size: window.Size): void => {
    this.apply(size.width);
  };

  private apply(widthPx: number): void {
    const next: string = Breakpoints.of(widthPx / this.density);
    if (next === this.current) return;
    this.current = next;
    if (this.callback !== undefined) this.callback(next);
  }
}
