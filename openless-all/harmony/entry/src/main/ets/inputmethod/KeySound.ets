import { media } from '@kit.MediaKit';
import { audio } from '@kit.AudioKit';
import { InputMethodExtensionContext } from '@kit.IMEKit';

// One small local sound, shared across key presses. No recording or network.
export class KeySound {
  private pool?: media.SoundPool;
  private sound: number = -1;
  private loading?: Promise<void>;

  play(context: InputMethodExtensionContext): void {
    if (!this.loading) this.loading = this.load(context).catch(() => {
      this.pool?.release().catch(() => {});
      this.pool = undefined;
      this.sound = -1;
      this.loading = undefined;
    });
    this.loading.then(() => {
      if (this.pool && this.sound >= 0) this.pool.play(this.sound, { leftVolume: 0.18, rightVolume: 0.18 }).catch(() => {});
    }).catch(() => {});
  }

  private async load(context: InputMethodExtensionContext): Promise<void> {
    const pool = await media.createSoundPool(2, { usage: audio.StreamUsage.STREAM_USAGE_MUSIC, rendererFlags: 0 });
    this.pool = pool;
    let timer: number = -1;
    const ready = new Promise<void>((resolve) => {
      pool.on('loadComplete', () => { clearTimeout(timer); resolve(); });
      timer = setTimeout(() => { this.sound = -1; resolve(); }, 1500);
    });
    const fd = await context.resourceManager.getRawFd('ime_click.wav');
    try {
      this.sound = await pool.load(fd.fd, fd.offset, fd.length);
      await ready;
    } finally {
      clearTimeout(timer);
      await context.resourceManager.closeRawFd('ime_click.wav');
    }
  }
}
