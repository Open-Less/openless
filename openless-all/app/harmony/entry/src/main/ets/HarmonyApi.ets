import { backgroundTaskManager } from '@kit.BackgroundTasksKit';
import { common, abilityAccessCtrl } from '@kit.AbilityKit';
import { audio } from '@kit.AudioKit';
import { fileIo as fs, picker } from '@kit.CoreFileKit';
import { webview } from '@kit.ArkWeb';
import { asset } from '@kit.AssetStoreKit';
import { util } from '@kit.ArkTS';
import { BusinessError, pasteboard } from '@kit.BasicServicesKit';
import nativeHost from 'libopenless_harmony.so';
import { restoreHarmonyCredentials } from './CredentialVault';
import { testRime } from './inputmethod/RimeClient';
import { VoiceClient, VoiceResult } from './inputmethod/VoiceClient';
import { publishVoiceConfiguration } from './VoiceConfiguration';

interface Revisions { preferencesRevision: number; stylePackRevision: number; historyRevision: number; vocabularyRevision: number; }
interface SiwcLaunch { attemptId: string; url: string; }
interface SiwcLaunchReply { ok: boolean; value: SiwcLaunch; error?: string; }
interface SiwcState { state: string; }
interface SiwcStateReply { ok: boolean; value: SiwcState; }
interface SiwcPublicLaunch { attemptId: string; }
interface SiwcCancelArgs { channelId: string; action: string; }
interface RevisionReply { ok: boolean; value: Revisions; }
interface CredentialChange { account: string; provider?: string | null; value: string | null; }
interface CredentialChangesReply { ok: boolean; value: CredentialChange[]; }
interface NetworkReply { ok: boolean; value: NetworkState; }
interface NetworkState { online: boolean; latencyMs: number | null; }
interface SystemState { microphone: string; deviceCount: number; online: boolean; dataDir: string; cacheDir: string; }
interface Reply { ok: boolean; value?: Object | null; error?: string; }
interface ArchiveReply { ok: boolean; value: string; error?: string; }
interface DeviceInfo { name: string; isDefault: boolean; }
interface DictationEvent { phase: string; level: number; error: string; }
interface VoiceCredentialStatus { pipelineMode: string; asrConfigured: boolean; omniConfigured: boolean; }
interface VoiceCredentialReply { ok: boolean; value: VoiceCredentialStatus; }
class CommandArgs { channelId: string = ''; action: string = ''; registrationId: string = '';  deviceName: string = ''; pane: string = ''; account: string = ''; provider?: string; value: string = ''; fileName: string = ''; message: string = ''; url: string = ''; id: string = ''; zipPath: string = ''; targetPath: string = ''; text: string = ''; }

export class HarmonyApi {
  private logs: string[] = [];
  private siwcDelay?: number;
  private siwcEpoch: number = 0;
  private siwcChannel: string = '';
  private endSiwcDelay(): void {
    this.siwcEpoch++;
    const delay = this.siwcDelay; this.siwcDelay = undefined;
    if (delay !== undefined) { try { backgroundTaskManager.cancelSuspendDelay(delay); } catch (_) {} }
  }
  private async cancelSiwc(channel: string): Promise<void> {
    const args: SiwcCancelArgs = {channelId: channel, action: 'cancel'};
    await nativeHost.invokeAsync('siwc_action', JSON.stringify(args));
  }
  private async beginSiwc(args: CommandArgs, argsJson: string): Promise<string> {
    if (this.siwcChannel) {
      await this.cancelSiwc(this.siwcChannel);
    }
    this.endSiwcDelay();
    this.siwcChannel = args.channelId;
    const channel = args.channelId;
    const epoch = this.siwcEpoch;
    const delay = backgroundTaskManager.requestSuspendDelay('ChatGPT authorization', () => {
      if (epoch !== this.siwcEpoch) return;
      this.cancelSiwc(channel).catch(() => {});
      this.endSiwcDelay();
    });
    this.siwcDelay = delay.requestId;
    try {
      const launch: SiwcLaunchReply = JSON.parse(await nativeHost.invokeAsync('__siwc_begin', argsJson));
      if (epoch !== this.siwcEpoch) {
        const reply: Reply = {ok: false, error: 'siwc: {"code":"cancelled"}'};
        return JSON.stringify(reply);
      }
      if (!launch.ok) {
        this.endSiwcDelay();
        const reply: Reply = {ok: false, error: launch.error || 'SIWC authorization failed'};
        return JSON.stringify(reply);
      }
      try { await this.openExternal(launch.value.url); }
      catch (_) { throw new Error('siwc: {"code":"browser_failed"}'); }
      const value: SiwcPublicLaunch = {attemptId: launch.value.attemptId};
      const reply: Reply = {ok: true, value: value};
      return JSON.stringify(reply);
    } catch (_) {
      if (epoch === this.siwcEpoch) {
        this.endSiwcDelay();
        await this.cancelSiwc(channel);
      }
      throw new Error('siwc: {"code":"browser_failed"}');
    }
  }

  appendLog(message: string): void {
    this.logs.push(new Date().toISOString() + ' ' + message);
    if (this.logs.length > 400) this.logs.shift();
  }

  async openExternal(url: string): Promise<void> {
    if (!this.context) throw new Error('应用上下文未就绪');
    if (!/^https?:\/\//i.test(url)) throw new Error('外部链接必须是 HTTP 或 HTTPS');
    await this.context.startAbility({action: 'ohos.want.action.viewData', uri: url, entities: ['entity.system.browsable']});
  }

  private async exportLog(fileName: string): Promise<string | null> {
    if (!this.context) throw new Error('应用上下文未就绪');
    const document = new picker.DocumentViewPicker(this.context);
    const options = new picker.DocumentSaveOptions();
    options.newFileNames = [fileName || 'openless-harmony.log'];
    const uris = await document.save(options);
    if (!uris.length) return null;
    const file = fs.openSync(uris[0], fs.OpenMode.READ_WRITE | fs.OpenMode.CREATE);
    try { fs.writeSync(file.fd, this.logs.join('\n')); } finally { fs.closeSync(file); }
    return uris[0];
  }

  private revisions?: Revisions;
  private nativeQueue: Promise<void> = Promise.resolve();

  private async notifyBackendChange(): Promise<void> {
    const snapshot: RevisionReply = JSON.parse(await nativeHost.invokeAsync('__revision_snapshot', '{}'));
    if (snapshot.ok) {
      if (this.revisions && JSON.stringify(this.revisions) !== JSON.stringify(snapshot.value)) {
        this.controller?.runJavaScript("window.dispatchEvent(new CustomEvent('openless:harmony-backend-change'))").catch(() => {});
      }
      this.revisions = snapshot.value;
    }
  }
  private async siwcNative(command: string, args: string): Promise<string> {
    // OAuth has its own durable native vault; never wait behind ordinary settings writes.
    const wire = await nativeHost.invokeAsync(command, args);
    await this.notifyBackendChange();
    return wire;
  }

  private async native(command: string, args: string): Promise<string> {
    const task = this.nativeQueue.then(async (): Promise<string> => {
      const wire = await nativeHost.invokeAsync(command, args);
      const changes: CredentialChangesReply = JSON.parse(await nativeHost.invokeAsync('__credential_writes', '{}'));
      if (!changes.ok) throw new Error('凭据持久化队列读取失败');
      for (const record of changes.value) {
        const entry = new CommandArgs();
        entry.account = record.account;
        entry.provider = record.provider || undefined;
        entry.value = record.value || '';
        await this.persistCredential(entry);
      }
      if (changes.value.length) await nativeHost.invokeAsync('__ack_credential_writes', '{}');
      await this.notifyBackendChange();
      await publishVoiceConfiguration();
      return wire;
    });
    this.nativeQueue = task.then(() => {}, () => {});
    return task;
  }

  private async pickZip(save: boolean, fileName: string): Promise<string | null> {
    if (!this.context) throw new Error('应用上下文未就绪');
    const document = new picker.DocumentViewPicker(this.context);
    if (save) {
      const options = new picker.DocumentSaveOptions();
      options.newFileNames = [fileName || 'style-pack.zip'];
      const files = await document.save(options);
      return files.length ? files[0] : null;
    }
    const options = new picker.DocumentSelectOptions();
    options.maxSelectNumber = 1;
    options.fileSuffixFilters = ['.zip'];
    const files = await document.select(options);
    return files.length ? files[0] : null;
  }

  private async importZip(args: CommandArgs): Promise<string> {
    if (!this.context) throw new Error('应用上下文未就绪');
    const target = this.context.cacheDir + '/openless/import-style-pack.zip';
    const source = fs.openSync(args.zipPath, fs.OpenMode.READ_ONLY);
    try {
      if (fs.statSync(source.fd).size > 512 * 1024) throw new Error('风格包 ZIP 最大为 512 KiB');
      const destination = fs.openSync(target, fs.OpenMode.CREATE | fs.OpenMode.READ_WRITE | fs.OpenMode.TRUNC);
      try { fs.copyFileSync(source.fd, destination.fd); } finally { fs.closeSync(destination); }
    } finally { fs.closeSync(source); }
    args.zipPath = target;
    return await this.native('import_style_pack_from_zip', JSON.stringify(args));
  }

  private async exportZip(args: CommandArgs): Promise<string> {
    if (!this.context) throw new Error('应用上下文未就绪');
    const uri = args.targetPath;
    args.targetPath = this.context.cacheDir + '/openless/export-style-pack.zip';
    const wire = await this.native('export_style_pack_to_zip', JSON.stringify(args));
    const result: Reply = JSON.parse(wire);
    if (!result.ok) return wire;
    const source = fs.openSync(args.targetPath, fs.OpenMode.READ_ONLY);
    try {
      const destination = fs.openSync(uri, fs.OpenMode.READ_WRITE | fs.OpenMode.CREATE | fs.OpenMode.TRUNC);
      try { fs.copyFileSync(source.fd, destination.fd); } finally { fs.closeSync(destination); }
    } finally { fs.closeSync(source); }
    const reply: Reply = {ok: true, value: uri};
    return JSON.stringify(reply);
  }

  private async downloadArchive(argsJson: string): Promise<void> {
    const args: CommandArgs = JSON.parse(argsJson);
    if (!args.targetPath) throw new Error('请选择风格包保存位置');
    const wire: ArchiveReply = JSON.parse(await this.native('__marketplace_download_archive', argsJson));
    if (!wire.ok) throw new Error(wire.error || '风格包下载失败');
    const bytes = new util.Base64Helper().decodeSync(wire.value);
    const destination = fs.openSync(args.targetPath, fs.OpenMode.CREATE | fs.OpenMode.READ_WRITE | fs.OpenMode.TRUNC);
    try { fs.writeSync(destination.fd, bytes.buffer); } finally { fs.closeSync(destination); }
  }

  private sequence: number = 0;
  private results: Map<string, string> = new Map();

  request(command: string, args: string): string {
    const id = String(++this.sequence);
    this.invokeAsync(command, args).then((result: string) => { this.results.set(id, result); });
    return id;
  }

  takeResult(id: string): string {
    const result = this.results.get(id);
    if (result === undefined) return '';
    this.results.delete(id);
    return result;
  }

  private context?: common.UIAbilityContext;
  private controller?: webview.WebviewController;
  private capturer?: audio.AudioCapturer;
  private lastLevelEvent: number = 0;
  private voice: VoiceClient = new VoiceClient();
  private voiceEpoch: number = 0;
  private voicePhase: string = 'idle';
  private voiceLevelAt: number = 0;

  private publishVoice(phase: string, level: number = 0, error: string = ''): void {
    this.voicePhase = phase;
    const state: DictationEvent = { phase: phase, level: level, error: error };
    this.controller?.runJavaScript("window.dispatchEvent(new CustomEvent('openless:harmony-dictation',{detail:" +
      JSON.stringify(state) + '}))').catch(() => {});
  }

  private async startDictation(): Promise<void> {
    if (!this.context) throw new Error('应用上下文未就绪');
    if (this.voicePhase !== 'idle') throw new Error('请先结束当前录音');
    const epoch = ++this.voiceEpoch;
    this.publishVoice('starting');
    try {
      await this.stopMonitor();
      if (epoch !== this.voiceEpoch) throw new Error('录音已取消');
      if (this.microphoneStatus() !== 'granted' && await this.requestMicrophone() !== 'granted') {
        throw new Error('未获得麦克风权限，请在权限页面授权');
      }
      if (epoch !== this.voiceEpoch) throw new Error('录音已取消');
      await this.voice.start(this.context, (level: number) => {
        if (epoch !== this.voiceEpoch || !['starting', 'recording'].includes(this.voicePhase) ||
          Date.now() - this.voiceLevelAt < 100) return;
        this.voiceLevelAt = Date.now();
        this.publishVoice('recording', level);
      }, (message: string) => {
        if (epoch === this.voiceEpoch) { this.voiceEpoch++; this.publishVoice('idle', 0, message); }
      });
      if (epoch !== this.voiceEpoch) throw new Error('录音已取消');
      this.publishVoice('recording');
    } catch (error) {
      if (epoch === this.voiceEpoch) { this.voiceEpoch++; this.publishVoice('idle', 0, String(error)); }
      throw new Error((error as Error).message || String(error));
    }
  }

  private async stopDictation(): Promise<VoiceResult> {
    if (this.voicePhase !== 'recording') throw new Error('当前没有可停止的录音');
    const epoch = this.voiceEpoch;
    this.publishVoice('processing');
    try {
      const result = await this.voice.stop();
      if (epoch !== this.voiceEpoch) throw new Error('录音已取消');
      this.voiceEpoch++;
      this.publishVoice('idle');
      this.controller?.runJavaScript("window.dispatchEvent(new CustomEvent('openless:harmony-backend-change'))").catch(() => {});
      return result;
    } catch (error) {
      if (epoch === this.voiceEpoch) { this.voiceEpoch++; this.publishVoice('idle', 0, String(error)); }
      throw new Error((error as Error).message || String(error));
    }
  }

  private async cancelDictation(): Promise<void> {
    const epoch = ++this.voiceEpoch;
    this.publishVoice('cancelling');
    try { await this.voice.cancel(); } finally { if (epoch === this.voiceEpoch) this.publishVoice('idle'); }
  }

  configure(context: common.UIAbilityContext, controller: webview.WebviewController): void {
    this.context = context;
    this.controller = controller;
  }

  private bytes(value: string): Uint8Array { return new util.TextEncoder().encodeInto(value); }
  private decode(value: Uint8Array): string { return util.TextDecoder.create('utf-8').decodeWithStream(value); }
  private alias(args: CommandArgs): Uint8Array { return this.bytes(JSON.stringify({account: args.account, provider: args.provider})); }

  async restoreCredentials(): Promise<void> {
    const restored = await restoreHarmonyCredentials();
    await publishVoiceConfiguration();
    const status: VoiceCredentialReply = JSON.parse(await nativeHost.invokeAsync('get_credentials', '{}'));
    if (status.ok) console.info('[OpenLess Main Voice] restored=' + restored + ' mode=' + status.value.pipelineMode +
      ' asrConfigured=' + status.value.asrConfigured + ' omniConfigured=' + status.value.omniConfigured);
  }

  private async persistCredential(args: CommandArgs): Promise<void> {
    if (!args.account) throw new Error('凭据字段不能为空');
    const attributes = new Map<asset.Tag, asset.Value>();
    attributes.set(asset.Tag.ALIAS, this.alias(args));
    attributes.set(asset.Tag.SECRET, this.bytes(args.value));
    attributes.set(asset.Tag.DATA_LABEL_NORMAL_1, this.bytes('openless.credentials'));
    attributes.set(asset.Tag.ACCESSIBILITY, asset.Accessibility.DEVICE_UNLOCKED);
    attributes.set(asset.Tag.CONFLICT_RESOLUTION, asset.ConflictResolution.OVERWRITE);
    if (!args.value) {
      const query = new Map<asset.Tag, asset.Value>();
      query.set(asset.Tag.ALIAS, this.alias(args));
      try { await asset.remove(query); } catch (error) { if ((error as BusinessError).code !== 24000002) throw new Error(String(error)); }
    } else {
      await asset.add(attributes);
    }

  }

  invoke(command: string, args: string): string {
    if (command.startsWith('__siwc')) {
      const reply: Reply = {ok: false, error: 'Native-only SIWC operation'};
      return JSON.stringify(reply);
    }
    return nativeHost.invoke(command, args);
  }

  private microphoneStatus(): string {
    if (!this.context) throw new Error('应用上下文未就绪');
    const token = this.context.applicationInfo.accessTokenId;
    if (token === undefined) throw new Error('应用权限标识未就绪');
    const status = abilityAccessCtrl.createAtManager().checkAccessTokenSync(token, 'ohos.permission.MICROPHONE');
    return status === abilityAccessCtrl.GrantStatus.PERMISSION_GRANTED ? 'granted' : 'indeterminate';
  }

  private devices(): audio.AudioDeviceDescriptors {
    return audio.getAudioManager().getRoutingManager().getDevicesSync(audio.DeviceFlag.INPUT_DEVICES_FLAG);
  }

  private deviceName(device: audio.AudioDeviceDescriptor): string {
    return device.displayName || device.name || ('麦克风 ' + device.id);
  }

  private async requestMicrophone(): Promise<string> {
    if (!this.context) throw new Error('应用上下文未就绪');
    const result = await abilityAccessCtrl.createAtManager().requestPermissionsFromUser(this.context, ['ohos.permission.MICROPHONE']);
    return result.authResults[0] === 0 ? 'granted' : 'denied';
  }

  private async stopMonitor(): Promise<void> {
    const capturer = this.capturer;
    this.capturer = undefined;
    if (capturer) {
      try { await capturer.stop(); } finally { await capturer.release(); }
    }
  }

  private async startMonitor(name: string): Promise<void> {
    if (this.voicePhase !== 'idle') throw new Error('请先结束当前录音');
    await this.stopMonitor();
    if (this.microphoneStatus() !== 'granted' && await this.requestMicrophone() !== 'granted') {
      throw new Error('未获得麦克风权限，请在权限页面授权');
    }
    const capturer = await audio.createAudioCapturer({
      streamInfo: { samplingRate: audio.AudioSamplingRate.SAMPLE_RATE_16000, channels: audio.AudioChannel.CHANNEL_1, sampleFormat: audio.AudioSampleFormat.SAMPLE_FORMAT_S16LE, encodingType: audio.AudioEncodingType.ENCODING_TYPE_RAW },
      capturerInfo: { source: audio.SourceType.SOURCE_TYPE_MIC, capturerFlags: 0 }
    });
    this.capturer = capturer;
    try {
      if (name) {
        const selected = this.devices().filter((device: audio.AudioDeviceDescriptor) => this.deviceName(device) === name);
        if (selected.length === 0) throw new Error('选中的麦克风已断开');
        const preferred = audio.getAudioManager().getRoutingManager().getPreferredInputDeviceForCapturerInfoSync({source: audio.SourceType.SOURCE_TYPE_MIC, capturerFlags: 0});
        if (!preferred.some((device: audio.AudioDeviceDescriptor) => device.id === selected[0].id)) throw new Error('当前系统使用其他输入设备，请在系统中切换麦克风或选择系统默认');
      }
      capturer.on('readData', (buffer: ArrayBuffer) => {
        const now = Date.now();
        if (now - this.lastLevelEvent < 100) return;
        this.lastLevelEvent = now;
        const pcm = new Int16Array(buffer);
        let sum = 0;
        for (let i = 0; i < pcm.length; i++) sum += pcm[i] * pcm[i];
        const level = pcm.length ? Math.min(1, Math.sqrt(sum / pcm.length) / 32768 * 4) : 0;
        this.controller?.runJavaScript("window.dispatchEvent(new CustomEvent('openless:harmony-level',{detail:" + level + '}))').catch(() => {});
      });
      await capturer.start();
      console.info('[OpenLess Audio] microphone monitor started');
    } catch (error) {
      await this.stopMonitor();
      throw new Error(String(error));
    }
  }

  async invokeAsync(command: string, argsJson: string): Promise<string> {
    try {
      if (command.startsWith('__siwc')) throw new Error('Native-only SIWC operation');
      const args: CommandArgs = JSON.parse(argsJson);
      let value: Object | null = null;
      switch (command) {
        case 'siwc_models': return await this.siwcNative(command, argsJson);
        case 'siwc_action': {
          if (['sign_in', 'add_account', 'enable_usage'].includes(args.action)) return await this.beginSiwc(args, argsJson);
          const wire = await this.siwcNative(command, argsJson);
          if (args.action === 'cancel' || args.action === 'disconnect') this.endSiwcDelay();
          return wire;
        }
        case 'siwc_status': {
          const wire = await this.siwcNative(command, argsJson);
          const state: SiwcStateReply = JSON.parse(wire);
          if (state.ok && state.value.state !== 'authorizing' && args.channelId === this.siwcChannel) this.endSiwcDelay();
          return wire;
        }
        case 'start_dictation': await this.startDictation(); break;
        case 'stop_dictation': value = await this.stopDictation(); break;
        case 'cancel_dictation': await this.cancelDictation(); break;
        case 'check_network': {
          const wire = await this.native('check_network', '{}');
          const reply: NetworkReply = JSON.parse(wire);
          console.info('[OpenLess Network] online=' + reply.value.online + ' latencyMs=' + reply.value.latencyMs);
          return wire;
        }
        case 'copy_text_to_clipboard':
        case 'copy_text': await pasteboard.getSystemPasteboard().setData(pasteboard.createData(pasteboard.MIMETYPE_TEXT_PLAIN, args.text)); break;
        case '__test_rime_roundtrip': {
          if (!this.context) throw new Error('应用上下文未就绪');
          value = await testRime(this.context.filesDir, this.context.resourceManager, this.context.cacheDir);
          break;
        }
        case 'open_external_url': await this.openExternal(args.url); break;
        case 'marketplace_download': await this.downloadArchive(argsJson); break;
        case 'export_error_log':
        case 'export_error_log_to_downloads': value = await this.exportLog(args.fileName); break;
        case 'export_harmony_error_log': value = await this.exportLog(args.fileName); break;
        case 'log_client_error': this.appendLog(args.message); break;
        case 'get_harmony_status': {
          if (!this.context) throw new Error('应用上下文未就绪');
          const network: NetworkReply = JSON.parse(await this.native('check_network', '{}'));
          const online = network.ok && network.value.online;
          const systemState: SystemState = {microphone: this.microphoneStatus(), deviceCount: this.devices().length, online: online, dataDir: this.context.filesDir + '/openless', cacheDir: this.context.cacheDir + '/openless'};
          value = systemState;
          break;
        }
        case 'pick_style_pack_zip': value = await this.pickZip(false, ''); break;
        case 'pick_style_pack_zip_target': value = await this.pickZip(true, args.fileName); break;
        case 'import_style_pack_from_zip': return await this.importZip(args);
        case 'export_style_pack_to_zip': return await this.exportZip(args);
        case 'list_microphone_devices':
          value = this.devices().map((device: audio.AudioDeviceDescriptor, index: number): DeviceInfo => ({name: this.deviceName(device), isDefault: index === 0}));
          break;
        case 'check_microphone_permission': value = this.microphoneStatus(); break;
        case 'request_microphone_permission': value = await this.requestMicrophone(); break;
        case 'trigger_microphone_prompt': await this.requestMicrophone(); break;
        case 'check_accessibility_permission': value = 'notApplicable'; break;
        case 'open_system_settings': {
          if (!this.context) throw new Error('应用上下文未就绪');
          await abilityAccessCtrl.createAtManager().requestPermissionOnSetting(this.context, ['ohos.permission.MICROPHONE']);
          break;
        }
        case 'start_microphone_level_monitor': await this.startMonitor(args.deviceName || ''); break;
        case 'stop_microphone_level_monitor': await this.stopMonitor(); break;
        default: return await this.native(command, argsJson);
      }
      const reply: Reply = {ok: true, value: value};
      return JSON.stringify(reply);
    } catch (error) {
      const reply: Reply = {ok: false, error: String(error)};
      return JSON.stringify(reply);
    }
  }

  async release(): Promise<void> {
    try { await this.cancelDictation(); } finally { await this.stopMonitor(); }
  }
}
