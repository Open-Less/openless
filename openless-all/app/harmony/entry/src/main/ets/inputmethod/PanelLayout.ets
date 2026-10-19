import { inputMethodEngine, InputMethodExtensionContext } from '@kit.IMEKit';
import { display } from '@kit.ArkUI';
import { preferences } from '@kit.ArkData';
import { imeStorage } from './InputSession';

// These are IME panel modes, not application overlay windows.
class PanelLayout {
  private panel?: inputMethodEngine.Panel;
  private settings?: preferences.Preferences;
  private mode: string = 'standard';
  private scale: number = 1;
  private x: number = 0.5;
  private y: number = 0.7;
  private width: number = 0;
  private height: number = 0;
  private queue: Promise<void> = Promise.resolve();
  private dragX: number = 0;
  private dragY: number = 0;
  private nextX: number = 0;
  private nextY: number = 0;
  private moving: boolean = false;
  private geometry: string = '';
  private flag: inputMethodEngine.PanelFlag = inputMethodEngine.PanelFlag.FLG_FIXED;

  async attach(panel: inputMethodEngine.Panel, context: InputMethodExtensionContext): Promise<void> {
    this.panel = panel;
    this.geometry = '';
    this.flag = inputMethodEngine.PanelFlag.FLG_FIXED;
    try {
      this.settings = await preferences.getPreferences(context, { name: 'ime_panel_layout' });
      const saved = await this.settings.get('mode', 'standard');
      this.mode = saved === 'split' || saved === 'floating' || saved === 'onehand' ? saved : 'standard';
      this.scale = this.clamp(Number(await this.settings.get('scale', 1)), 0.8, 1.2);
      this.x = this.clamp(Number(await this.settings.get('x', 0.5)), 0, 1);
      this.y = this.clamp(Number(await this.settings.get('y', 0.7)), 0, 1);
    } catch (_) { console.warn('[OpenLess IME] layout preferences unavailable'); }
    try { await this.apply(); }
    catch (_) { this.mode = 'standard'; await this.apply(); }
  }

  detach(): void { this.panel = undefined; }

  private clamp(value: number, min: number, max: number): number {
    return Number.isFinite(value) ? Math.max(min, Math.min(max, value)) : min;
  }

  private async apply(): Promise<void> {
    const panel = this.panel;
    if (!panel) return;
    const screen = display.getDefaultDisplaySync();
    const density = screen.densityPixels;
    const floating = this.mode === 'floating';
    const geometry = screen.width + ':' + screen.height + ':' + density + ':' + this.mode + ':' + this.scale;
    // A resize can itself emit display/keyboard notifications. Do not resize
    // again unless the screen or requested layout actually changed.
    if (this.geometry === geometry) return;
    this.geometry = '';
    // Keep all five rows visible in landscape as well as portrait.
    const requestedHeight = floating ? Math.round(304 * this.scale) : 328;
    const maxPanelVp = Math.min(requestedHeight, Math.floor(screen.height * 0.65 / density));
    const showNumberRow = !floating && maxPanelVp >= 300;
    const rows = showNumberRow ? 5 : 4;
    this.width = floating ? Math.round(Math.min(screen.width * 0.9, 400 * this.scale * density)) : screen.width;
    const split = this.mode === 'split';
    const gap = this.mode === 'standard' ? 7 : 5;
    const columns = split ? 12 : 10;
    const gaps = split ? 10 : 9;
    // Fill the panel width, but keep a compact row height. On a wide tablet
    // this deliberately produces wider keys instead of a taller keyboard.
    const keyboardWidth = this.mode === 'onehand' ? Math.min(420, this.width / density * 0.72) - 12 : this.width / density - 12;
    const keyWidth = (keyboardWidth - gaps * gap) / columns;
    const keyHeight = Math.max(12, Math.floor(Math.min(floating ? keyWidth : maxPanelVp,
      (maxPanelVp - 66 - (floating ? 26 : 0) - (rows - 1) * 7) / rows)));
    const fittedHeight = rows * keyHeight + (rows - 1) * 7 + 66 + (floating ? 26 : 0);
    this.height = Math.round(Math.min(maxPanelVp, fittedHeight) * density);
    const flag = floating ? inputMethodEngine.PanelFlag.FLG_FLOATING : inputMethodEngine.PanelFlag.FLG_FIXED;
    if (flag !== this.flag) { panel.changeFlag(flag); this.flag = flag; }
    await panel.resize(this.width, this.height);
    if (floating) await panel.moveTo(Math.round(this.x * (screen.width - this.width)),
      Math.round(this.y * (screen.height - this.height)));
    if (this.panel !== panel) return;
    this.geometry = geometry;
    imeStorage.setOrCreate('keyboardLayout', this.mode);
    imeStorage.setOrCreate('keyHeight', keyHeight);
    imeStorage.setOrCreate('keyWidth', keyWidth);
    imeStorage.setOrCreate('keyboardWidth', keyboardWidth);
    imeStorage.setOrCreate('showNumberRow', showNumberRow);
    console.info('[OpenLess IME] layout=' + this.mode + ' width=' + this.width + ' height=' + this.height);
  }

  refresh(): Promise<void> {
    this.queue = this.queue.then(() => this.apply()).catch(() => {
      imeStorage.setOrCreate('status', '键盘尺寸调整失败，请重新选择布局');
    });
    return this.queue;
  }

  select(mode: string): void {
    if (!['standard', 'split', 'floating', 'onehand'].includes(mode)) return;
    console.info('[OpenLess IME] layout requested=' + mode + ' panel=' + !!this.panel);
    this.queue = this.queue.then(async (): Promise<void> => {
      const previous = this.mode;
      this.mode = mode;
      try { await this.apply(); }
      catch (error) {
        console.error('[OpenLess IME] layout failed: ' + JSON.stringify(error));
        this.mode = previous; await this.apply(); throw new Error('layout failed');
      }
      await this.save();
    }).catch(() => { imeStorage.setOrCreate('status', '布局切换失败，请重试'); });
  }

  resize(delta: number): void {
    this.queue = this.queue.then(async (): Promise<void> => {
      if (this.mode !== 'floating') return;
      this.scale = this.clamp(this.scale + delta, 0.8, 1.2);
      await this.apply();
      await this.save();
    }).catch(() => { imeStorage.setOrCreate('status', '悬浮键盘调整失败'); });
  }

  beginMove(): void { this.dragX = this.x; this.dragY = this.y; }

  drag(dx: number, dy: number): void {
    if (this.mode !== 'floating' || !this.panel) return;
    const screen = display.getDefaultDisplaySync();
    this.nextX = this.clamp(this.dragX + dx * screen.densityPixels / Math.max(1, screen.width - this.width), 0, 1);
    this.nextY = this.clamp(this.dragY + dy * screen.densityPixels / Math.max(1, screen.height - this.height), 0, 1);
    if (this.moving) return;
    this.moving = true;
    this.queue = this.queue.then(async (): Promise<void> => {
      try {
        while (this.mode === 'floating' && this.panel) {
          this.x = this.nextX; this.y = this.nextY;
          await this.panel.moveTo(Math.round(this.x * (screen.width - this.width)),
            Math.round(this.y * (screen.height - this.height)));
          if (this.x === this.nextX && this.y === this.nextY) break;
        }
      } finally { this.moving = false; }
    }).catch(() => { this.moving = false; });
  }

  endMove(): void { this.queue = this.queue.then(() => this.save()).catch(() => {}); }

  private async save(): Promise<void> {
    try {
      if (!this.settings) return;
      await this.settings.put('mode', this.mode);
      await this.settings.put('scale', this.scale);
      await this.settings.put('x', this.x);
      await this.settings.put('y', this.y);
      await this.settings.flush();
    } catch (_) { console.warn('[OpenLess IME] layout preferences not saved'); }
  }
}

export const panelLayout: PanelLayout = new PanelLayout();
