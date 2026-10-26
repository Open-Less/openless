import { Labels } from '../common/Labels';
import { C, R, S, T } from '../common/Tokens';
import { Card, EmptyState, GhostButton, PageTitle, Pill, SectionHeader } from '../components/Common';
import { Nav } from '../components/Nav';
import { Api } from '../data/Api';
import { PlatformCapabilities, SettingsSnapshot, UserPreferences } from '../data/Types';

const LANGUAGES: string[] = ['zh-CN', 'en', 'ja', 'ko'];
const LANGUAGE_NAMES: string[] = ['简体中文', 'English', '日本語', '한국어'];

/**
 * 翻译 / 速记 / 语音提问 / 模型设置.
 *
 * These four share the "tool" shape: one primary control, a short result area and
 * a card that explains what the device can actually do. Anything the HarmonyOS
 * host cannot do yet is stated explicitly instead of faking a control.
 */
@Component
export struct TranslationPage {
  @StorageLink('bp') bp: string = 'md';

  @State private prefs: UserPreferences = {};
  @State private error: string = '';
  @State private busy: boolean = false;

  aboutToAppear(): void {
    this.reload();
  }

  private async reload(): Promise<void> {
    try {
      const snapshot: SettingsSnapshot = await Api.settings();
      this.prefs = snapshot.preferences;
      this.error = '';
    } catch (error) {
      this.error = (error as Error).message;
    }
  }

  private async write(pointer: string, value: string): Promise<void> {
    this.busy = true;
    try {
      const snapshot: SettingsSnapshot = await Api.writeString(pointer, value);
      this.prefs = snapshot.preferences;
    } catch (error) {
      this.error = (error as Error).message;
    } finally {
      this.busy = false;
    }
  }

  private languageName(code: string): string {
    for (let index = 0; index < LANGUAGES.length; index++) {
      if (LANGUAGES[index] === code) return LANGUAGE_NAMES[index];
    }
    return code;
  }

  build() {
    Column() {
      PageTitle({ title: '翻译', subtitle: '把说的话直接翻译成目标语言' })

      if (this.error.length > 0) {
        Text(this.error).fontSize(T.footnote).fontColor(C.danger()).padding({ bottom: S.sm })
      }

      Scroll() {
        Column({ space: S.md }) {
          Card() {
            Column({ space: S.sm }) {
              SectionHeader({ title: '目标语言', hint: '翻译模式会把转写结果翻译成这里的语言' })
              Row({ space: S.sm }) {
                ForEach(LANGUAGES, (code: string) => {
                  Text(this.languageName(code))
                    .fontSize(T.footnote)
                    .fontColor(this.prefs.translationTargetLanguage === code ? C.textOnAccent() : C.textSecondary())
                    .padding({ left: S.md, right: S.md, top: S.xs, bottom: S.xs })
                    .backgroundColor(this.prefs.translationTargetLanguage === code ? C.accent() : C.sunken())
                    .borderRadius(R.pill)
                    .onClick(() => {
                      if (!this.busy) this.write('/translationTargetLanguage', code);
                    })
                }, (code: string) => 'lang-' + code)
              }
              .width('100%')
            }
            .width('100%')
          }

          Card() {
            Column({ space: S.sm }) {
              SectionHeader({ title: '当前模式', hint: '点一下切换，也可以在设置里改默认值' })
              Row({ space: S.sm }) {
                ForEach(['raw', 'polish', 'translate', 'structured'], (mode: string) => {
                  Text(Labels.polishMode(mode))
                    .fontSize(T.footnote)
                    .fontColor(this.prefs.defaultMode === mode ? C.textOnAccent() : C.textSecondary())
                    .padding({ left: S.md, right: S.md, top: S.xs, bottom: S.xs })
                    .backgroundColor(this.prefs.defaultMode === mode ? C.accent() : C.sunken())
                    .borderRadius(R.pill)
                    .onClick(() => {
                      if (!this.busy) this.write('/defaultMode', mode);
                    })
                }, (mode: string) => 'tmode-' + mode)
              }
              .width('100%')
              Text(this.prefs.defaultMode === 'translate'
                ? '听写结束后会直接输出 ' + this.languageName(this.prefs.translationTargetLanguage ?? 'en') + ' 译文。'
                : '当前不是翻译模式；选「翻译」后听写结果会直接是译文。')
                .fontSize(T.footnote)
                .fontColor(C.textTertiary())
                .width('100%')
            }
            .width('100%')
          }

          Card() {
            Column({ space: S.xs }) {
              SectionHeader({ title: '怎么用' })
              Text('1. 选好目标语言与「翻译」模式；\n2. 回概览页按住麦克风说话；\n3. 上屏的就是译文，历史里会保留原文与译文两种文本。')
                .fontSize(T.callout)
                .fontColor(C.textSecondary())
                .lineHeight(24)
                .width('100%')
            }
            .width('100%')
          }
        }
        .width('100%')
        .padding({ bottom: S.xxl })
      }
      .scrollBar(BarState.Off)
      .layoutWeight(1)
      .align(Alignment.Top)
    }
    .width('100%')
    .height('100%')
    .padding({ left: S.gutter(this.bp), right: S.gutter(this.bp) })
  }
}

@Component
export struct QuickNotePage {
  @StorageLink('bp') bp: string = 'md';
  @StorageLink('dictPhase') phase: string = 'idle';
  @StorageLink('dictError') dictError: string = '';

  @State private note: string = '';
  @State private error: string = '';
  @State private busy: boolean = false;

  private async dictate(): Promise<void> {
    this.busy = true;
    this.error = '';
    try {
      if (this.phase === 'idle' || this.dictError.length > 0) {
        await Api.startDictation();
      } else {
        const voice = await Api.stopDictation();
        const text: string = voice.polishedText.length > 0 ? voice.polishedText : voice.rawText;
        this.note = this.note.length > 0 ? this.note + '\n' + text : text;
      }
    } catch (error) {
      this.error = (error as Error).message;
    } finally {
      this.busy = false;
    }
  }

  build() {
    Column() {
      PageTitle({ title: '速记', subtitle: '想到什么先记下来，稍后再整理' })

      Row({ space: S.sm }) {
        GhostButton({
          label: this.phase === 'recording' ? '结束并追加' : '语音追加',
          symbol: 'mic',
          usable: !this.busy,
          onTap: () => {
            this.dictate();
          }
        })
        GhostButton({
          label: '复制全部',
          symbol: 'copy',
          usable: this.note.length > 0,
          onTap: () => {
            Api.copyText(this.note);
          }
        })
        GhostButton({
          label: '清空',
          symbol: 'trash',
          usable: this.note.length > 0,
          onTap: () => {
            this.note = '';
          }
        })
      }
      .width('100%')
      .padding({ bottom: S.sm })

      if (this.error.length > 0 || (this.dictError.length > 0 && this.phase === 'idle')) {
        Text(this.error.length > 0 ? this.error : this.dictError)
          .fontSize(T.footnote)
          .fontColor(C.danger())
          .padding({ bottom: S.sm })
      }

      TextArea({ placeholder: '在这里写，或用上面的麦克风追加一段', text: this.note })
        .fontSize(T.body)
        .fontColor(C.textPrimary())
        .backgroundColor(C.surface())
        .borderRadius(R.lg)
        .padding(S.md)
        .layoutWeight(1)
        .width('100%')
        .onChange((value: string) => {
          this.note = value;
        })
    }
    .width('100%')
    .height('100%')
    .padding({ left: S.gutter(this.bp), right: S.gutter(this.bp), bottom: S.md })
  }
}

@Component
export struct QaPage {
  @StorageLink('bp') bp: string = 'md';
  @StorageLink('dictPhase') phase: string = 'idle';
  @StorageLink('dictError') dictError: string = '';

  @State private question: string = '';
  @State private error: string = '';
  @State private busy: boolean = false;
  @State private capabilities: PlatformCapabilities = { platform: 'harmony' };

  aboutToAppear(): void {
    Api.platformCapabilities().then((value: PlatformCapabilities) => {
      this.capabilities = value;
    }).catch((error: Error) => {
      this.error = error.message;
    });
  }

  private async dictate(): Promise<void> {
    this.busy = true;
    this.error = '';
    try {
      if (this.phase === 'idle' || this.dictError.length > 0) {
        await Api.startDictation();
      } else {
        const voice = await Api.stopDictation();
        this.question = voice.polishedText.length > 0 ? voice.polishedText : voice.rawText;
      }
    } catch (error) {
      this.error = (error as Error).message;
    } finally {
      this.busy = false;
    }
  }

  build() {
    Column() {
      PageTitle({ title: '问答', subtitle: '把问题说出来，先拿到干净的文本' })

      Scroll() {
        Column({ space: S.md }) {
          Card() {
            Column({ space: S.sm }) {
              SectionHeader({ title: '语音提问', hint: '鸿蒙端先把问题转写成文字' })
              Text(this.question.length > 0 ? this.question : '说一句话，转写结果会出现在这里')
                .fontSize(T.body)
                .fontColor(this.question.length > 0 ? C.textPrimary() : C.textTertiary())
                .lineHeight(26)
                .width('100%')
                .copyOption(CopyOptions.LocalDevice)
              Row({ space: S.sm }) {
                GhostButton({
                  label: this.phase === 'recording' ? '结束' : '开始说话',
                  symbol: 'mic',
                  usable: !this.busy,
                  onTap: () => {
                    this.dictate();
                  }
                })
                GhostButton({
                  label: '复制问题',
                  symbol: 'copy',
                  usable: this.question.length > 0,
                  onTap: () => {
                    Api.copyText(this.question);
                  }
                })
              }
              .width('100%')
              if (this.error.length > 0 || (this.dictError.length > 0 && this.phase === 'idle')) {
                Text(this.error.length > 0 ? this.error : this.dictError)
                  .fontSize(T.footnote)
                  .fontColor(C.danger())
              }
            }
            .width('100%')
          }

          Card() {
            Column({ space: S.xs }) {
              SectionHeader({ title: '回答在哪', hint: '能力随宿主平台不同' })
              Row({ space: S.sm }) {
                Pill({ text: this.capabilities.platform, tone: 'accent' })
                Pill({
                  text: this.capabilities.supportsVoiceEdit === true ? '支持语音编辑' : '不支持语音编辑',
                  tone: 'neutral'
                })
              }
              Text('桌面的「问任何问题」面板依赖浮窗与选区读取，鸿蒙宿主目前没有这些能力（supportsOverlay=false）。' +
                '这里的语音提问负责把问题转成文本，回答仍由桌面端或你常用的模型客户端完成。')
                .fontSize(T.footnote)
                .fontColor(C.textSecondary())
                .lineHeight(22)
                .width('100%')
              Row() {
                GhostButton({
                  label: '去设置模型',
                  onTap: () => Nav.go(Nav.SETTINGS)
                })
              }
              .width('100%')
              .justifyContent(FlexAlign.End)
            }
            .width('100%')
          }
        }
        .width('100%')
        .padding({ bottom: S.xxl })
      }
      .scrollBar(BarState.Off)
      .layoutWeight(1)
      .align(Alignment.Top)
    }
    .width('100%')
    .height('100%')
    .padding({ left: S.gutter(this.bp), right: S.gutter(this.bp) })
  }
}

@Component
export struct LocalAsrPage {
  @StorageLink('bp') bp: string = 'md';

  @State private capabilities: PlatformCapabilities = { platform: 'harmony' };
  @State private asr: string = '';
  @State private error: string = '';

  aboutToAppear(): void {
    Promise.all([Api.platformCapabilities(), Api.settings()])
      .then((values: Object[]) => {
        this.capabilities = values[0] as PlatformCapabilities;
        const snapshot = values[1] as SettingsSnapshot;
        this.asr = snapshot.preferences.activeAsrProvider ?? '';
      })
      .catch((error: Error) => {
        this.error = error.message;
      });
  }

  build() {
    Column() {
      PageTitle({ title: '模型设置', subtitle: '本地与云端模型的能力边界' })

      if (this.error.length > 0) {
        Text(this.error).fontSize(T.footnote).fontColor(C.danger()).padding({ bottom: S.sm })
      }

      Scroll() {
        Column({ space: S.md }) {
          Card() {
            Column({ space: S.sm }) {
              SectionHeader({ title: '本地模型', hint: '在设备上推理，不联网' })
              Row({ space: S.sm }) {
                Pill({
                  text: this.capabilities.supportsLocalAsr === true ? '支持本地转写' : '鸿蒙端暂不支持',
                  tone: this.capabilities.supportsLocalAsr === true ? 'success' : 'warning'
                })
                Pill({
                  text: this.capabilities.supportsInAppDictation === true ? '应用内听写可用' : '听写不可用',
                  tone: 'neutral'
                })
              }
              Text(this.capabilities.supportsLocalAsr === true
                ? '当前宿主可以在本地加载 ASR 模型。'
                : 'HarmonyOS 宿主目前只接入云端转写（supportsLocalAsr=false）：本地模型需要额外的 NPU 适配与模型分发，' +
                  '还没在这个版本里提供。听写仍然可用，走的是当前选中的云端引擎。')
                .fontSize(T.footnote)
                .fontColor(C.textSecondary())
                .lineHeight(22)
                .width('100%')
            }
            .width('100%')
          }

          Card() {
            Column({ space: S.sm }) {
              SectionHeader({ title: '当前转写引擎' })
              Text(Labels.provider(this.asr))
                .fontSize(T.headline)
                .fontColor(C.textPrimary())
              Row({ space: S.sm }) {
                GhostButton({
                  label: '更换引擎',
                  symbol: 'settings',
                  onTap: () => Nav.go(Nav.SETTINGS)
                })
              }
              .width('100%')
            }
            .width('100%')
          }

          if (this.capabilities.supportsLocalAsr !== true) {
            EmptyState({
              symbol: 'localasr',
              title: '本地模型还不在这台设备上',
              hint: '能力开关来自 Rust Core 的平台能力表；等宿主补上本地推理后，这一页会自动出现模型列表。'
            })
          }
        }
        .width('100%')
        .padding({ bottom: S.xxl })
      }
      .scrollBar(BarState.Off)
      .layoutWeight(1)
      .align(Alignment.Top)
    }
    .width('100%')
    .height('100%')
    .padding({ left: S.gutter(this.bp), right: S.gutter(this.bp) })
  }
}
