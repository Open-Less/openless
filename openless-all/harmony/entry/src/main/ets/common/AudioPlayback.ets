import { media } from '@kit.MediaKit';
import { fileIo as fs } from '@kit.CoreFileKit';
import { util } from '@kit.ArkTS';

/** One cancellable archived recording. A late player must never outlive its page. */
export class AudioPlayback {
  private player?: media.AVPlayer;
  private sourceFile?: fs.File;
  private playing: boolean = false;
  private generation: number = 0;

  isPlaying(): boolean {
    return this.playing;
  }

  async play(cacheDir: string, sessionId: string, base64: string, onState: (playing: boolean) => void): Promise<void> {
    const generation: number = ++this.generation;
    await this.releaseCurrent();
    if (generation !== this.generation) return;
    const path: string = cacheDir + '/rec-' + sessionId + '.wav';
    const marker: number = base64.indexOf(';base64,');
    const encoded: string = marker >= 0 ? base64.slice(marker + 8) : base64;
    const bytes: Uint8Array = new util.Base64Helper().decodeSync(encoded);
    const file = fs.openSync(path, fs.OpenMode.CREATE | fs.OpenMode.READ_WRITE | fs.OpenMode.TRUNC);
    try {
      fs.writeSync(file.fd, bytes.buffer);
    } finally {
      fs.closeSync(file);
    }
    const player = await media.createAVPlayer();
    if (generation !== this.generation) {
      await player.release();
      return;
    }
    this.player = player;
    const sourceFile = fs.openSync(path, fs.OpenMode.READ_ONLY);
    this.sourceFile = sourceFile;
    player.on('stateChange', (state: string) => {
      if (generation !== this.generation) return;
      if (state === 'completed' || state === 'error') {
        this.playing = false;
        onState(false);
      }
    });
    player.on('error', () => {
      if (generation !== this.generation) return;
      this.playing = false;
      onState(false);
    });
    try {
      player.fdSrc = { fd: sourceFile.fd, offset: 0, length: bytes.byteLength };
      await player.prepare();
      if (generation !== this.generation) return;
      await player.play();
      if (generation !== this.generation) return;
      this.playing = true;
      onState(true);
    } catch (error) {
      if (generation !== this.generation) return;
      await this.stop();
      onState(false);
      throw new Error((error as Error).message);
    }
  }

  async stop(): Promise<void> {
    this.generation++;
    await this.releaseCurrent();
  }

  private async releaseCurrent(): Promise<void> {
    const player = this.player;
    const sourceFile = this.sourceFile;
    this.player = undefined;
    this.sourceFile = undefined;
    this.playing = false;
    try {
      if (player !== undefined) {
        try {
          await player.stop();
        } catch (error) {
          // A created/preparing player cannot always stop, but must still release.
        }
        await player.release();
      }
    } finally {
      if (sourceFile !== undefined) fs.closeSync(sourceFile);
    }
  }
}
