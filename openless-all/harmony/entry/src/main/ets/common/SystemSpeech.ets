import { abilityAccessCtrl, common } from '@kit.AbilityKit';
import { speechRecognizer } from '@kit.CoreSpeechKit';
import { Store } from './Store';

export interface SystemSpeechSnapshot {
  supported: boolean;
  phase: string;
  text: string;
  error: string;
}

/** Independent, offline-only trial, NOT the core dictation provider.
 * Official contract (online: 1 means offline; recognitionMode: 0 captures audio):
 * https://developer.huawei.com/consumer/cn/doc/doccenter-references/api/hms-ai-speechrecognizer
 * No transcript is logged, uploaded, polished, inserted or persisted here.
 */
export class SystemSpeech {
  private engine: speechRecognizer.SpeechRecognitionEngine | undefined = undefined;
  private generation: number = 0;
  private sessionId: string = '';
  private deadline: number = -1;
  private disposed: boolean = false;
  private ownsMicrophone: boolean = false;
  private phase: string = 'idle';
  private text: string = '';
  private error: string = '';
  private supported: boolean = SystemSpeech.available();
  private onChange: (snapshot: SystemSpeechSnapshot) => void;

  constructor(onChange: (snapshot: SystemSpeechSnapshot) => void) {
    this.onChange = onChange;
  }

  static available(): boolean {
    try {
      return canIUse('SystemCapability.AI.SpeechRecognizer');
    } catch (error) {
      return false;
    }
  }

  snapshot(): SystemSpeechSnapshot {
    return { supported: this.supported, phase: this.phase, text: this.text, error: this.error };
  }

  private publish(): void {
    if (!this.disposed) this.onChange(this.snapshot());
  }

  private current(generation: number, sessionId: string): boolean {
    return !this.disposed && generation === this.generation && sessionId === this.sessionId;
  }

  private mainBusy(): boolean {
    const phase: string = AppStorage.get<string>('dictPhase') ?? 'idle';
    return AppStorage.get<boolean>('microphoneMonitorBusy') === true ||
      phase === 'starting' || phase === 'recording' || phase === 'processing' || phase === 'cancelling';
  }

  private foreground(): boolean {
    return AppStorage.get<boolean>('appForeground') !== false;
  }

  private clearDeadline(): void {
    if (this.deadline >= 0) {
      clearTimeout(this.deadline);
      this.deadline = -1;
    }
  }

  private arm(generation: number, sessionId: string, delay: number, message: string): void {
    this.clearDeadline();
    this.deadline = setTimeout(() => {
      if (this.current(generation, sessionId)) this.fail(message);
    }, delay);
  }

  /** Invalidate callbacks before cancel/shutdown, which may synchronously call listeners. */
  private release(cancel: boolean): void {
    this.generation++;
    const sessionId: string = this.sessionId;
    this.sessionId = '';
    this.clearDeadline();
    const engine: speechRecognizer.SpeechRecognitionEngine | undefined = this.engine;
    this.engine = undefined;
    if (engine !== undefined) {
      if (cancel && sessionId.length > 0) {
        try { engine.cancel(sessionId); } catch (error) { /* shutdown still runs */ }
      }
      try { engine.shutdown(); } catch (error) { /* no sensitive diagnostic logging */ }
    }
    if (this.ownsMicrophone) {
      this.ownsMicrophone = false;
      Store.put<boolean>('systemSpeechBusy', false);
    }
  }

  private fail(message: string): void {
    this.release(true);
    this.phase = 'error';
    this.text = '';
    this.error = message;
    this.publish();
  }

  private errorText(code: number): string {
    if (code === 1002200012 || code === 201) return '未获得麦克风权限，请在系统设置允许后重试。';
    if (code === 1002200006) return '系统语音服务正忙，请结束其他录音后重试。';
    if (code === 1002200001) return '无法准备离线中文识别，设备可能缺少系统语音资源。';
    return '系统离线识别暂时不可用，请稍后重试（' + code.toString() + '）。';
  }

  private listener(generation: number, sessionId: string): speechRecognizer.RecognitionListener {
    const listener: speechRecognizer.RecognitionListener = {
      onStart: (id: string, message: string): void => {
        if (!this.current(generation, id) || id !== sessionId || this.phase !== 'starting') return;
        this.phase = 'listening';
        this.arm(generation, id, 25000, '本次试用已超时，麦克风已释放，请重新开始。');
        this.publish();
      },
      onEvent: (id: string, code: number, message: string): void => {
        if (!this.current(generation, id) || id !== sessionId) return;
        if (code === 3 && this.phase === 'listening') {
          this.phase = 'finishing';
          this.arm(generation, id, 10000, '等待离线识别结果超时，请重新开始。');
          this.publish();
        }
      },
      onResult: (id: string, result: speechRecognizer.SpeechRecognitionResult): void => {
        if (!this.current(generation, id) || id !== sessionId) return;
        this.text = result.result;
        this.publish();
      },
      onComplete: (id: string, message: string): void => {
        if (!this.current(generation, id) || id !== sessionId) return;
        this.release(false);
        this.phase = 'done';
        this.error = this.text.length > 0 ? '' : '没有识别到文字，可重新试用。';
        this.publish();
      },
      onError: (id: string, code: number, message: string): void => {
        if (this.current(generation, id) && id === sessionId) this.fail(this.errorText(code));
      }
    };
    return listener;
  }

  /** The only permission entry point: invoked explicitly by the trial start button. */
  async start(context: common.UIAbilityContext): Promise<void> {
    if (this.disposed || this.ownsMicrophone) return;
    if (!this.supported) {
      this.fail('这台设备未提供系统离线中文识别能力，不会切换到云端。');
      return;
    }
    if (!this.foreground() || this.mainBusy() || AppStorage.get<boolean>('systemSpeechBusy') === true) {
      this.fail('请先结束其他听写，并在前台打开本页后重试。');
      return;
    }
    const generation: number = ++this.generation;
    const sessionId: string = 'offline_' + Date.now().toString() + '_' + generation.toString();
    this.sessionId = sessionId;
    this.phase = 'starting';
    this.text = '';
    this.error = '';
    this.ownsMicrophone = true;
    Store.put<boolean>('systemSpeechBusy', true);
    this.arm(generation, sessionId, 30000, '准备语音超时，请确认麦克风权限后重试。');
    this.publish();
    try {
      const manager: abilityAccessCtrl.AtManager = abilityAccessCtrl.createAtManager();
      const token: number = context.applicationInfo.accessTokenId;
      let granted: boolean = manager.checkAccessTokenSync(token, 'ohos.permission.MICROPHONE') ===
        abilityAccessCtrl.GrantStatus.PERMISSION_GRANTED;
      if (!granted) {
        const response = await manager.requestPermissionsFromUser(context, ['ohos.permission.MICROPHONE']);
        granted = response.authResults.length > 0 &&
          response.authResults[0] === abilityAccessCtrl.GrantStatus.PERMISSION_GRANTED;
      }
      if (!this.current(generation, sessionId)) return;
      if (!granted) {
        this.fail('未获得麦克风权限，请在系统设置允许后重试。');
        return;
      }
      if (!this.foreground() || this.mainBusy()) {
        this.fail('其他听写已启动或页面不在前台，本次试用已取消。');
        return;
      }
      this.arm(generation, sessionId, 10000, '准备离线识别超时，设备可能缺少系统语音资源。');
      const params: speechRecognizer.CreateEngineParams = {
        language: 'zh-CN', online: 1, extraParams: { locate: 'CN', recognizerMode: 'short' }
      };
      const engine: speechRecognizer.SpeechRecognitionEngine = await speechRecognizer.createEngine(params);
      if (!this.current(generation, sessionId)) {
        try { engine.shutdown(); } catch (error) { /* late creation must not acquire audio */ }
        return;
      }
      this.engine = engine;
      if (!this.foreground() || this.mainBusy()) {
        this.fail('其他听写已启动或页面不在前台，本次试用已取消。');
        return;
      }
      engine.setListener(this.listener(generation, sessionId));
      const startParams: speechRecognizer.StartParams = {
        sessionId: sessionId,
        audioInfo: { audioType: 'pcm', sampleRate: 16000, soundChannel: 1, sampleBit: 16 },
        extraParams: { recognitionMode: 0, maxAudioDuration: 20000 }
      };
      engine.startListening(startParams);
    } catch (error) {
      if (this.current(generation, sessionId)) {
        this.fail('无法启动系统离线识别，请检查权限及设备语音资源后重试。');
      }
    }
  }

  finish(): void {
    if (this.phase !== 'listening' || this.engine === undefined) return;
    this.phase = 'finishing';
    this.arm(this.generation, this.sessionId, 10000, '等待离线识别结果超时，麦克风已释放。');
    this.publish();
    try { this.engine.finish(this.sessionId); } catch (error) { this.fail('结束识别失败，请重新试用。'); }
  }

  cancel(): void {
    this.release(true);
    this.phase = 'idle';
    this.text = '';
    this.error = '';
    this.publish();
  }

  dispose(): void {
    this.disposed = true;
    this.release(true);
    this.onChange = (snapshot: SystemSpeechSnapshot): void => {};
  }
}
