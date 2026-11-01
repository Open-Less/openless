import { media } from '@kit.MediaKit';
import { fileIo as fs } from '@kit.CoreFileKit';
import { util } from '@kit.ArkTS';

/**
 * Plays one archived recording from the local cache.
 *
 * The core hands back base64 WAV (`read_audio_recording`); the player needs a file
 * descriptor, so the payload is written once into the cache directory and handed to
 * `AVPlayer` with an explicit offset/length. Replay of the same session reuses the
 * file.
 */
export class AudioPlayback {
  private player?: media.AVPlayer;
  private path: string = '';
  private playing: boolean = false;

  isPlaying(): boolean {
    return this.playing;
  }

  /** Writes the payload and starts playback; `onState` reports 'playing' / 'idle'. */
  async play(cacheDir: string, sessionId: string, base64: string, onState: (playing: boolean) => void): Promise<void> {
    await this.stop();
    this.path = cacheDir + '/rec-' + sessionId + '.wav';
    const bytes: Uint8Array = new util.Base64Helper().decodeSync(base64);
    const file = fs.openSync(this.path, fs.OpenMode.CREATE | fs.OpenMode.READ_WRITE | fs.OpenMode.TRUNC);
    try {
      fs.writeSync(file.fd, bytes.buffer);
    } finally {
      fs.closeSync(file);
    }
    const size: number = bytes.byteLength;
    const player = await media.createAVPlayer();
    this.player = player;
    this.playing = true;
    onState(true);
    player.on('stateChange', (state: string) => {
      if (state === 'completed' || state === 'error') {
        this.playing = false;
        onState(false);
      }
    });
    player.on('error', () => {
      this.playing = false;
      onState(false);
    });
    player.fdSrc = { fd: fs.openSync(this.path, fs.OpenMode.READ_ONLY).fd, offset: 0, length: size };
    await player.prepare();
    await player.play();
  }

  async stop(): Promise<void> {
    const player = this.player;
    this.player = undefined;
    this.playing = false;
    if (player === undefined) return;
    try {
      await player.stop();
      await player.release();
    } catch (error) {
      console.warn('[OpenLess Audio] release failed');
    }
  }
}
