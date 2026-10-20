import { abilityAccessCtrl, common } from '@kit.AbilityKit';
import { audio } from '@kit.AudioKit';
import nativeHost from 'libopenless_harmony.so';
import { restoreHarmonyCredentials } from '../CredentialVault';
import { restoreVoiceConfiguration } from '../VoiceConfiguration';

interface NativeReply { ok: boolean; error?: string; }
interface StartReply extends NativeReply { value: string; }
export interface VoiceResult { rawText: string; polishedText: string; }
interface StopReply extends NativeReply { value: VoiceResult; }
interface VoiceSettings { microphoneDeviceName: string; }
interface SettingsReply extends NativeReply { value: VoiceSettings; }
interface CredentialStatus { pipelineMode: string; asrConfigured: boolean; omniConfigured: boolean; }
interface CredentialReply extends NativeReply { value: CredentialStatus; }

class VoiceRun {
  session: string = '';
  capturer?: audio.AudioCapturer;
  cancelled: boolean = false;
  accepting: boolean = false;
  pendingBytes: number = 0;
  delivery: Promise<void> = Promise.resolve();
  startup: Promise<void> = Promise.resolve();
  failure: string = '';
  stage: string = 'permission';
}

function requireSuccess(wire: string): void {
  const reply: NativeReply = JSON.parse(wire);
  if (!reply.ok) throw new Error(reply.error || '语音后端调用失败');
}

// AudioKit owns capture; openless-core owns sessions, providers and history.
// Every run retains its own delivery queue, even when cancellation races stop.
export class VoiceClient {
  private readonly fromKeyboard: boolean;
  constructor(fromKeyboard: boolean = false) { this.fromKeyboard = fromKeyboard; }
  private run?: VoiceRun;
  private cleanup: Promise<void> = Promise.resolve();

  async setPolishMode(context: common.Context, mode: string): Promise<void> {
    requireSuccess(nativeHost.initBackend(context.filesDir + '/openless', context.cacheDir + '/openless'));
    if (this.fromKeyboard) await restoreVoiceConfiguration();
    requireSuccess(await nativeHost.invokeAsync('set_default_polish_mode', JSON.stringify({ mode: mode })));
  }

  async start(context: common.Context, level: (value: number) => void,
    failed: (message: string) => void): Promise<void> {
    if (this.run) throw new Error('录音尚未结束');
    const run = new VoiceRun();
    this.run = run;
    run.startup = this.begin(run, context, level, failed);
    try { await run.startup; } catch (error) {
      console.warn('[OpenLess Voice] startup failed stage=' + run.stage);
      if (this.run === run) this.run = undefined;
      try { await this.dispose(run, true); } finally { throw new Error((error as Error).message); }
    }
  }

  private check(run: VoiceRun): void {
    if (run.cancelled || this.run !== run) throw new Error('录音已取消');
  }

  private async begin(run: VoiceRun, context: common.Context,
    level: (value: number) => void, failed: (message: string) => void): Promise<void> {
    await this.cleanup;
    this.check(run);
    const token = context.applicationInfo.accessTokenId;
    if (token === undefined || abilityAccessCtrl.createAtManager().checkAccessTokenSync(token,
      'ohos.permission.MICROPHONE') !== abilityAccessCtrl.GrantStatus.PERMISSION_GRANTED) {
      throw new Error('请打开 OpenLess 设置，在权限与数据中授权麦克风');
    }
    run.stage = 'backend_init';
    requireSuccess(nativeHost.initBackend(context.filesDir + '/openless', context.cacheDir + '/openless'));
    console.info('[OpenLess Voice] backend dataDir=' + context.filesDir + '/openless');
    run.stage = 'configuration_restore';
    if (this.fromKeyboard) await restoreVoiceConfiguration();
    this.check(run);
    run.stage = 'backend_reload';
    requireSuccess(await nativeHost.invokeAsync('__reload_voice_backend', '{}'));
    this.check(run);
    run.stage = 'credential_restore';
    const restored = await restoreHarmonyCredentials();
    this.check(run);
    run.stage = 'credential_status';
    const status: CredentialReply = JSON.parse(await nativeHost.invokeAsync('get_credentials', '{}'));
    if (status.ok) {
      console.info('[OpenLess Voice] restored=' + restored + ' mode=' + status.value.pipelineMode +
        ' asrConfigured=' + status.value.asrConfigured + ' omniConfigured=' + status.value.omniConfigured);
    } else { console.warn('[OpenLess Voice] credential status unavailable'); }
    run.stage = 'settings';
    const settings: SettingsReply = JSON.parse(await nativeHost.invokeAsync('get_settings', '{}'));
    if (!settings.ok) throw new Error(settings.error || '麦克风设置读取失败');
    const name = settings.value.microphoneDeviceName;
    if (name) {
      const routing = audio.getAudioManager().getRoutingManager();
      const selected = routing.getDevicesSync(audio.DeviceFlag.INPUT_DEVICES_FLAG)
        .filter((device: audio.AudioDeviceDescriptor) => (device.displayName || device.name || ('麦克风 ' + device.id)) === name);
      if (!selected.length) throw new Error('选中的麦克风已断开，请在设置中选择系统默认');
      const preferred = routing.getPreferredInputDeviceForCapturerInfoSync({source: audio.SourceType.SOURCE_TYPE_MIC, capturerFlags: 0});
      if (!preferred.some((device: audio.AudioDeviceDescriptor) => device.id === selected[0].id)) {
        throw new Error('当前系统使用其他麦克风，请切换系统输入设备或选择系统默认');
      }
    }
    this.check(run);
    run.stage = 'session_start';
    const reply: StartReply = JSON.parse(await nativeHost.invokeAsync('start_dictation', '{}'));
    if (!reply.ok) throw new Error(reply.error || '请在设置中配置可用的语音识别渠道');
    run.session = reply.value;
    this.check(run);
    run.stage = 'capturer_create';
    const capturer = await audio.createAudioCapturer({
      streamInfo: { samplingRate: audio.AudioSamplingRate.SAMPLE_RATE_16000,
        channels: audio.AudioChannel.CHANNEL_1, sampleFormat: audio.AudioSampleFormat.SAMPLE_FORMAT_S16LE,
        encodingType: audio.AudioEncodingType.ENCODING_TYPE_RAW },
      capturerInfo: { source: audio.SourceType.SOURCE_TYPE_MIC, capturerFlags: 0 }
    });
    run.capturer = capturer;
    this.check(run);
    run.accepting = true;
    capturer.on('readData', (buffer: ArrayBuffer) => {
      if (!run.accepting || run.cancelled) return;
      if (buffer.byteLength % 2 !== 0 || run.pendingBytes + buffer.byteLength > 65536) {
        run.failure = '音频传输未能及时完成，请重新录音';
        run.accepting = false;
        failed(run.failure);
        this.cancel().catch(() => {});
        return;
      }
      const samples = new Int16Array(buffer);
      let sum = 0;
      for (let i = 0; i < samples.length; i++) sum += samples[i] * samples[i];
      level(samples.length ? Math.min(1, Math.sqrt(sum / samples.length) / 8192) : 0);
      // Copy AudioKit's callback buffer before returning ownership to AudioKit.
      for (let offset = 0; offset < buffer.byteLength; offset += 32768) {
        const frame = buffer.slice(offset, Math.min(offset + 32768, buffer.byteLength));
        run.pendingBytes += frame.byteLength;
        run.delivery = run.delivery.then(async (): Promise<void> => {
          if (!run.cancelled && !run.failure) requireSuccess(await nativeHost.feedPcm(run.session, frame));
        }).catch(() => {
          if (!run.cancelled && !run.failure) {
            run.failure = '音频传输失败，请重新录音';
            failed(run.failure);
            this.cancel().catch(() => {});
          }
        }).then(() => { run.pendingBytes -= frame.byteLength; });
      }
    });
    run.stage = 'capturer_start';
    await capturer.start();
    this.check(run);
    run.stage = 'recording';
    console.info('[OpenLess Voice] recording started');
  }

  private async release(run: VoiceRun): Promise<void> {
    const capturer = run.capturer;
    run.capturer = undefined;
    if (capturer) {
      try { if (capturer.state === audio.AudioState.STATE_RUNNING) await capturer.stop(); } finally {
        run.accepting = false;
        try { capturer.off('readData'); } finally { await capturer.release(); }
      }
    }
    run.accepting = false;
  }

  private async dispose(run: VoiceRun, cancel: boolean): Promise<void> {
    try { await this.release(run); } finally {
      await run.delivery;
      if (cancel && run.session) {
        requireSuccess(await nativeHost.invokeAsync('cancel_dictation', JSON.stringify({session: run.session})));
      }
    }
  }

  async stop(): Promise<VoiceResult> {
    const run = this.run;
    if (!run) throw new Error('录音已结束');
    try {
      await run.startup;
      this.check(run);
      await this.dispose(run, false);
      this.check(run);
      if (run.failure) throw new Error(run.failure);
      const reply: StopReply = JSON.parse(await nativeHost.invokeAsync('stop_dictation', JSON.stringify({session: run.session})));
      this.check(run);
      if (!reply.ok) throw new Error(reply.error || '识别失败，请检查渠道设置');
      return reply.value;
    } catch (error) {
      try { if (!run.cancelled) await this.dispose(run, true); }
      finally { throw new Error((error as Error).message); }
    } finally { if (this.run === run) this.run = undefined; }
  }

  async cancel(): Promise<void> {
    const run = this.run;
    if (!run) return;
    run.cancelled = true;
    run.accepting = false;
    this.run = undefined;
    const cleanup = run.startup.catch(() => {}).then(async (): Promise<void> => { await this.dispose(run, true); });
    // Wait for release without permanently poisoning starts after a system
    // capture error. Core still rejects a genuinely busy recording.
    this.cleanup = cleanup.catch(() => {});
    await cleanup;
  }
}
