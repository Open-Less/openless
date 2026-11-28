import { common } from '@kit.AbilityKit';
import { SystemSpeech, SystemSpeechSnapshot } from '../../common/SystemSpeech';
import { Leading, Palette, Space, Type } from '../../common/Theme';
import { Banner, PrimaryButton, SectionTitle, Surface, TextButton } from '../Kit';

/** Explicit trial only. Mounting/foregrounding never starts the microphone. */
@Component
export struct SystemSpeechCard {
  @StorageLink('appForeground') @Watch('onForeground') foreground: boolean = true;
  @StorageLink('dictPhase') @Watch('onDictation') dictPhase: string = 'idle';
  @State private snapshot: SystemSpeechSnapshot = {
    supported: SystemSpeech.available(), phase: 'idle', text: '', error: ''
  };
  onCopy: (text: string) => void = () => {};
  private speech: SystemSpeech | undefined = undefined;
  private mounted: boolean = false;

  aboutToAppear(): void {
    this.mounted = true;
    this.speech = new SystemSpeech((snapshot: SystemSpeechSnapshot): void => {
      if (this.mounted && this.foreground) this.snapshot = snapshot;
    });
    this.snapshot = this.speech.snapshot();
  }

  aboutToDisappear(): void {
    this.mounted = false;
    this.speech?.dispose();
    this.speech = undefined;
  }

  private onForeground(): void {
    if (!this.foreground) this.speech?.cancel();
    else if (this.mounted && this.speech !== undefined) this.snapshot = this.speech.snapshot();
  }

  private onDictation(): void {
    if (this.mainBusy() && this.active()) this.speech?.cancel();
  }

  private mainBusy(): boolean {
    return this.dictPhase === 'starting' || this.dictPhase === 'recording' ||
      this.dictPhase === 'processing' || this.dictPhase === 'cancelling';
  }

  private active(): boolean {
    return this.snapshot.phase === 'starting' || this.snapshot.phase === 'listening' ||
      this.snapshot.phase === 'finishing';
  }

  private actionTitle(): string {
    if (this.snapshot.phase === 'starting') return '正在准备';
    if (this.snapshot.phase === 'listening') return '结束识别';
    if (this.snapshot.phase === 'finishing') return '正在识别';
    return '试用离线中文识别';
  }

  private begin(): void {
    if (this.snapshot.phase === 'listening') this.speech?.finish();
    else this.speech?.start(this.getUIContext().getHostContext() as common.UIAbilityContext);
  }

  build() {
    Surface({ inset: Space.md }) {
      Column({ space: Space.sm }) {
        SectionTitle({ title: '系统离线语音试用', hint: 'Core Speech Kit · 中文 · 在设备上识别' })
        Text('独立试用，不写入历史、不润色、不自动输入，也不会替换主听写引擎。仅点击开始时申请麦克风权限。')
          .fontSize(Type.caption).lineHeight(Leading.caption).fontColor(Palette.text3())
        if (!this.snapshot.supported) {
          Banner({ title: '设备暂不支持', text: '系统未提供语音识别能力，不会自动回落到云端。' })
        }
        PrimaryButton({
          label: this.actionTitle(),
          usable: this.snapshot.supported && this.foreground && !this.mainBusy() &&
            this.snapshot.phase !== 'starting' && this.snapshot.phase !== 'finishing',
          onTap: () => { this.begin(); }
        })
        if (this.active()) {
          TextButton({ label: '取消试用', tone: 'danger', onTap: () => { this.speech?.cancel(); } })
        }
        if (this.mainBusy()) {
          Text('请先结束主听写，再试用系统离线语音。')
            .fontSize(Type.caption).lineHeight(Leading.caption).fontColor(Palette.text3())
        }
        if (this.snapshot.error.length > 0) {
          Banner({ title: '试用提示', text: this.snapshot.error })
        }
        if (this.snapshot.text.length > 0) {
          SectionTitle({ title: this.active() ? '实时识别' : '试用结果' })
          Text(this.snapshot.text)
            .fontSize(Type.body).lineHeight(Leading.body).fontColor(Palette.text1())
            .width('100%').copyOption(CopyOptions.LocalDevice)
          if (this.snapshot.phase === 'done') {
            TextButton({ label: '复制结果', onTap: () => { this.onCopy(this.snapshot.text); } })
          }
        }
      }
      .width('100%').alignItems(HorizontalAlign.Start)
    }
  }
}
