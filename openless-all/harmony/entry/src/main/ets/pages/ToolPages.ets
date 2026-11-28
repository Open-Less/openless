import { Fmt } from '../common/Format';
import { Labels } from '../common/Labels';
import { Layout, Leading, Palette, Space, Type } from '../common/Theme';
import {
  Banner,
  EmptyHint,
  ErrorNote,
  Group,
  InputField,
  PageScaffold,
  PrimaryButton,
  RowItem,
  SectionTitle,
  Segmented,
  SettingSwitch,
  SheetTitle,
  Skeleton,
  Surface,
  Tag,
  TextButton,
  TonalButton,
  notify
} from '../components/Kit';
import { Nav } from '../components/Nav';
import { Api } from '../data/Api';
import { PlatformCapabilities, SettingsSnapshot, UserPreferences } from '../data/Types';

/** Canonical native names expected by `openless-core::UserPreferences.translation_target_language`. */
const LANGUAGES: string[] = ['简体中文', 'English', '日本語', '한국어'];
const LANGUAGE_NAMES: string[] = ['简体中文', 'English', '日本語', '한국어'];
/** Valid `openless-core::PolishMode` variants (`raw | light | structured | formal`). */
const MODES: string[] = ['raw', 'light', 'structured', 'formal'];

/** One dictation result kept on the 问答 page for the current session. */
interface QaTurn {
  key: string;
  text: string;
  at: string;
}

/**
 * Tools v2: 翻译 / 问答 / 速记 / 模型设置.
 *
 * The four share one shape: a PageScaffold title, one primary action, an editable
 * draft, and a card that states what the host can actually do. Everything is
 * composed from Kit (Surface / Group / RowItem / Segmented / InputField / ...) so
 * the tool pages look like the rest of the app; the business wiring is the same
 * as v1 (settings pointers, repolish, dictation, clipboard).
 */
@Component
export struct TranslationPage {
  @StorageLink('bp') bp: string = 'md';
  @StorageLink('phoneLayout') phoneLayout: boolean = true;

  @State private prefs: UserPreferences = {};
  @State private lastTarget: string = 'English';
  @State private error: string = '';
  @State private busy: boolean = false;
  @State private draft: string = '';
  @State private output: string = '';
  @State private working: boolean = false;

  aboutToAppear(): void {
    this.reload();
  }

  private isWide(): boolean {
    // Portrait tablets present the phone UI, so they stay single-column too.
    return !this.phoneLayout && (this.bp === Layout.LG || this.bp === Layout.XL);
  }

  private async reload(): Promise<void> {
    try {
      const snapshot: SettingsSnapshot = await Api.settings();
      this.prefs = snapshot.preferences;
      const saved: string = (snapshot.preferences.translationTargetLanguage ?? '').trim();
      if (saved.length > 0) this.lastTarget = saved;
      this.error = '';
    } catch (error) {
      this.error = (error as Error).message;
    }
  }

  private async writeString(pointer: string, value: string): Promise<void> {
    if (this.busy) return;
    this.busy = true;
    try {
      const snapshot: SettingsSnapshot = await Api.writeString(pointer, value);
      this.prefs = snapshot.preferences;
      if (pointer === '/translationTargetLanguage' && value.trim().length > 0) {
        this.lastTarget = value.trim();
      }
      this.error = '';
    } catch (error) {
      this.error = (error as Error).message;
    } finally {
      this.busy = false;
    }
  }

  private async writePolishMode(mode: string): Promise<void> {
    if (this.busy) return;
    this.busy = true;
    try {
      await Api.setDefaultPolishMode(mode);
      const snapshot: SettingsSnapshot = await Api.settings();
      this.prefs = snapshot.preferences;
      this.error = '';
    } catch (error) {
      this.error = (error as Error).message;
    } finally {
      this.busy = false;
    }
  }

  private translationOn(): boolean {
    return (this.prefs.translationTargetLanguage ?? '').trim().length > 0;
  }

  private async setTranslationEnabled(enabled: boolean): Promise<void> {
    const target: string = enabled ? (this.lastTarget.length > 0 ? this.lastTarget : 'English') : '';
    await this.writeString('/translationTargetLanguage', target);
  }

  private targetIndex(): number {
    const raw: string = (this.prefs.translationTargetLanguage ?? '').trim();
    const target: string = raw.length > 0 ? raw : this.lastTarget;
    for (let index = 0; index < LANGUAGES.length; index++) {
      if (LANGUAGES[index] === target) return index;
    }
    return 1;
  }

  private targetName(): string {
    return this.translationOn() ? LANGUAGE_NAMES[this.targetIndex()] : '已关闭';
  }

  private modeIndex(): number {
    const mode: string = this.prefs.defaultMode ?? 'light';
    for (let index = 0; index < MODES.length; index++) {
      if (MODES[index] === mode) return index;
    }
    return 1;
  }

  private modeLabels(): string[] {
    const labels: string[] = [];
    for (let index = 0; index < MODES.length; index++) {
      labels.push(Labels.polishMode(MODES[index]));
    }
    return labels;
  }

  private modeHint(): string {
    if (this.translationOn()) {
      return '目标语言已设为 ' + this.targetName() + '，开启翻译听写时将输出译文并保留原文。';
    }
    return '当前未开启翻译目标；开启下方开关或选择目标语言后即可用于翻译听写。';
  }

  /** One-shot rewrite through the active model, same path as dictation. */
  private async runTransform(): Promise<void> {
    const input: string = this.draft.trim();
    if (input.length === 0 || this.working) return;
    this.working = true;
    this.error = '';
    try {
      this.output = await Api.repolish(input, this.prefs.activeStylePackId ?? '');
      notify('已按「' + Labels.polishMode(this.prefs.defaultMode) + '」生成');
    } catch (error) {
      this.error = (error as Error).message;
    } finally {
      this.working = false;
    }
  }

  private copyOutput(): void {
    if (this.output.length === 0) return;
    Api.copyText(this.output);
    notify('已复制结果');
  }

  build() {
    PageScaffold({
      title: '翻译目标',
      actionLabel: '刷新',
      actionSymbol: 'refresh',
      onAction: () => {
        this.reload();
      }
    }) {
      if (this.error.length > 0) {
        ErrorNote({
          message: this.error,
          onRetry: () => {
            this.reload();
          }
        })
      }
      if (this.isWide()) {
        Row({ space: Space.md }) {
          Column({ space: Space.md }) {
            this.languageCard()
            this.modeCard()
            this.usageCard()
          }
          .layoutWeight(5)
          .alignItems(HorizontalAlign.Start)

          Column({ space: Space.md }) {
            this.tryCard()
            this.resultCard()
          }
          .layoutWeight(4)
          .alignItems(HorizontalAlign.Start)
        }
        .width('100%')
        .alignItems(VerticalAlign.Top)
      } else {
        Column({ space: Space.md }) {
          this.languageCard()
          this.modeCard()
          this.tryCard()
          this.resultCard()
          this.usageCard()
        }
        .width('100%')
      }
    }
  }

  @Builder
  languageCard() {
    Surface({ inset: Space.md }) {
      Column({ space: Space.sm }) {
        SectionTitle({ title: '目标语言', hint: '翻译模式会把转写结果翻译成这里的语言' })
        Segmented({
          items: LANGUAGE_NAMES,
          selected: this.targetIndex(),
          onSelect: (index: number) => {
            this.writeString('/translationTargetLanguage', LANGUAGES[index]);
          }
        })
        Row({ space: Space.xs }) {
          Tag({ text: '源语言 · 自动识别', tone: 'neutral' })
          Tag({ text: this.targetName(), tone: 'accent' })
        }
        .width('100%')
      }
      .width('100%')
    }
  }

  @Builder
  modeCard() {
    Surface({ inset: Space.md }) {
      Column({ space: Space.sm }) {
        SectionTitle({ title: '默认润色模式', hint: '点一下切换，与输入法语音和全局默认风格同步' })
        Segmented({
          items: this.modeLabels(),
          selected: this.modeIndex(),
          onSelect: (index: number) => {
            this.writePolishMode(MODES[index]);
          }
        })
        Text(this.modeHint())
          .fontSize(Type.label)
          .lineHeight(Leading.label)
          .fontColor(Palette.text3())
          .width('100%')
        SettingSwitch({
          label: '启用翻译目标语言',
          hint: '关掉后会清空目标语言，仅按所选润色模式输出',
          checked: this.translationOn(),
          showDivider: false,
          onToggle: (value: boolean) => {
            this.setTranslationEnabled(value);
          }
        })
      }
      .width('100%')
    }
  }

  @Builder
  tryCard() {
    Surface({ inset: Space.md }) {
      Column({ space: Space.sm }) {
        SectionTitle({ title: '试一下', hint: '走当前模型的一次性改写，不录音' })
        InputField({
          label: '原文',
          placeholder: '粘贴一段文字',
          value: this.draft,
          multiline: true,
          onInput: (value: string) => {
            this.draft = value;
          }
        })
        Row({ space: Space.sm }) {
          TonalButton({
            label: this.working ? '处理中…' : '按当前模式改写',
            symbol: 'sparkles',
            usable: !this.working && this.draft.trim().length > 0,
            onTap: () => {
              this.runTransform();
            }
          })
          TextButton({
            label: '清空',
            tone: 'quiet',
            usable: this.draft.length > 0 && !this.working,
            onTap: () => {
              this.draft = '';
            }
          })
        }
        .width('100%')

        if (this.working) {
          Skeleton({ rows: 1 })
        }
      }
      .width('100%')
    }
  }

  @Builder
  resultCard() {
    if (this.output.length > 0) {
      Surface({ inset: Space.md }) {
        Column({ space: Space.xs }) {
          Row({ space: Space.sm }) {
            Text('结果')
              .fontSize(Type.label)
              .lineHeight(Leading.label)
              .fontWeight(FontWeight.Medium)
              .fontColor(Palette.text2())
              .layoutWeight(1)
            TextButton({
              label: '复制',
              symbol: 'copy',
              onTap: () => {
                this.copyOutput();
              }
            })
          }
          .width('100%')

          Scroll() {
            Text(this.output)
              .fontSize(Type.body)
              .lineHeight(Leading.body)
              .fontColor(Palette.text1())
              .width('100%')
              .copyOption(CopyOptions.LocalDevice)
          }
          .width('100%')
          .constraintSize({ maxHeight: 280 })
          .align(Alignment.Top)
          .scrollBar(BarState.Auto)
        }
        .width('100%')
      }
    }
  }

  @Builder
  usageCard() {
    Surface({ tone: 'alt', inset: Space.md }) {
      Column({ space: Space.xs }) {
        SectionTitle({ title: '怎么用', hint: '翻译模式在听写链路上生效' })
        Text('1. 选好目标语言与「翻译」模式；\n2. 回概览页点一下麦克风说话；\n3. 上屏的就是译文，历史里保留原文与译文两种文本。')
          .fontSize(Type.label)
          .lineHeight(Leading.body)
          .fontColor(Palette.text2())
          .width('100%')
      }
      .width('100%')
    }
  }
}

@Component
export struct QaPage {
  @StorageLink('bp') bp: string = 'md';
  @StorageLink('phoneLayout') phoneLayout: boolean = true;
  @StorageLink('dictPhase') phase: string = 'idle';
  @StorageLink('dictError') dictError: string = '';

  @State private question: string = '';
  @State private error: string = '';
  @State private busy: boolean = false;
  @State private capabilities: PlatformCapabilities = { platform: 'harmony' };
  @State private turns: QaTurn[] = [];

  aboutToAppear(): void {
    this.loadCapabilities();
  }

  private isWide(): boolean {
    // Portrait tablets present the phone UI, so they stay single-column too.
    return !this.phoneLayout && (this.bp === Layout.LG || this.bp === Layout.XL);
  }

  private async loadCapabilities(): Promise<void> {
    try {
      this.capabilities = await Api.platformCapabilities();
      this.error = '';
    } catch (error) {
      this.error = (error as Error).message;
    }
  }

  private async dictate(): Promise<void> {
    if (this.busy) return;
    this.busy = true;
    this.error = '';
    try {
      if (this.phase === 'idle' || this.dictError.length > 0) {
        await Api.startDictation();
      } else {
        const voice = await Api.stopDictation();
        const text: string = voice.polishedText.length > 0 ? voice.polishedText : voice.rawText;
        this.question = text;
        this.pushTurn(text);
      }
    } catch (error) {
      this.error = (error as Error).message;
    } finally {
      this.busy = false;
    }
  }

  /** Newest first, capped: this is a scratch pad, not persisted history. */
  private pushTurn(text: string): void {
    if (text.length === 0) return;
    const turn: QaTurn = {
      key: 'turn-' + Date.now().toString() + '-' + this.turns.length.toString(),
      text: text,
      at: new Date().toISOString()
    };
    const next: QaTurn[] = [turn];
    for (let index = 0; index < this.turns.length && index < 19; index++) {
      next.push(this.turns[index]);
    }
    this.turns = next;
  }

  private copyQuestion(): void {
    if (this.question.length === 0) return;
    Api.copyText(this.question);
    notify('已复制问题');
  }

  private phaseLabel(): string {
    if (this.phase === 'recording') return '结束并转写';
    if (this.phase === 'processing') return '正在转写';
    return '开始说话';
  }

  private errorText(): string {
    return this.error.length > 0 ? this.error : this.dictError;
  }

  build() {
    PageScaffold({
      title: '问答',
      subtitle: '把问题说出来，先拿到干净的文本',
      actionLabel: '设置模型',
      actionSymbol: 'settings',
      onAction: () => Nav.go(Nav.SETTINGS)
    }) {
      if (this.capabilities.supportsOverlay !== true) {
        this.answerBanner()
      }
      if (this.errorText().length > 0 && this.phase === 'idle') {
        ErrorNote({
          message: this.errorText(),
          onRetry: () => {
            this.loadCapabilities();
          }
        })
      }
      if (this.isWide()) {
        Row({ space: Space.md }) {
          Column({ space: Space.md }) {
            this.askCard()
            this.turnsCard()
          }
          .layoutWeight(5)
          .alignItems(HorizontalAlign.Start)

          Column({ space: Space.md }) {
            this.capabilityCard()
            this.usageCard()
          }
          .layoutWeight(4)
          .alignItems(HorizontalAlign.Start)
        }
        .width('100%')
        .alignItems(VerticalAlign.Top)
      } else {
        Column({ space: Space.md }) {
          this.askCard()
          this.turnsCard()
          this.capabilityCard()
          this.usageCard()
        }
        .width('100%')
      }
    }
  }

  @Builder
  answerBanner() {
    Banner({
      title: '回答不在这台设备上',
      text: '鸿蒙宿主没有浮窗与选区读取能力（supportsOverlay=false）。这一页负责把问题转成干净文本，回答交给桌面端或你常用的模型客户端。',
      tone: 'accent',
      action: '去设置模型',
      actionSymbol: 'settings',
      onAction: () => Nav.go(Nav.SETTINGS)
    })
  }

  @Builder
  askCard() {
    Surface({ inset: Space.md }) {
      Column({ space: Space.sm }) {
        SectionTitle({ title: '语音提问', hint: '转写在本机完成后直接落到输入框' })
        InputField({
          label: '问题',
          placeholder: '说一句话，或直接打字',
          value: this.question,
          multiline: true,
          onInput: (value: string) => {
            this.question = value;
          }
        })
        Flex({ wrap: FlexWrap.Wrap, justifyContent: FlexAlign.Start }) {
          PrimaryButton({
            label: this.phaseLabel(),
            usable: !this.busy && this.phase !== 'processing',
            onTap: () => {
              this.dictate();
            }
          })
          TonalButton({
            label: '复制问题',
            symbol: 'copy',
            usable: this.question.length > 0,
            onTap: () => {
              this.copyQuestion();
            }
          })
          TextButton({
            label: '清空',
            tone: 'quiet',
            usable: this.question.length > 0 && !this.busy,
            onTap: () => {
              this.question = '';
            }
          })
        }
        .width('100%')
      }
      .width('100%')
    }
  }

  @Builder
  turnsCard() {
    Group() {
      Column() {
        Row() {
          SectionTitle({ title: '本次会话', hint: '只留在这一页，离开就清空' })
        }
        .width('100%')
        .padding({ left: Space.md, right: Space.md, top: Space.sm, bottom: Space.xxs })

        if (this.turns.length === 0) {
          EmptyHint({ symbol: 'qa', title: '还没有提问记录', hint: '点「开始说话」，转写结果会留在这里' })
        } else {
          ForEach(this.turns, (turn: QaTurn, index: number) => {
            RowItem({
              title: Fmt.preview(turn.text, 44),
              subtitle: Fmt.relative(turn.at),
              value: '复制',
              showDivider: index < this.turns.length - 1,
              onTap: () => {
                this.question = turn.text;
                Api.copyText(turn.text);
                notify('已复制');
              }
            })
          }, (turn: QaTurn) => turn.key)
        }
      }
      .width('100%')
    }
  }

  @Builder
  capabilityCard() {
    Group() {
      Column() {
        Row() {
          SectionTitle({ title: '平台能力', hint: '能力表来自 Rust Core' })
        }
        .width('100%')
        .padding({ left: Space.md, right: Space.md, top: Space.sm, bottom: Space.xxs })

        RowItem({
          title: '平台',
          value: this.capabilities.platform,
          showDivider: true
        })
        RowItem({
          title: '浮窗与选区',
          subtitle: '桌面端的「问任何问题」面板依赖的能力',
          value: this.capabilities.supportsOverlay === true ? '支持' : '不支持',
          showDivider: true
        })
        RowItem({
          title: '语音编辑',
          value: this.capabilities.supportsVoiceEdit === true ? '支持' : '不支持',
          showDivider: true
        })
        RowItem({
          title: '应用内听写',
          value: this.capabilities.supportsInAppDictation === true ? '可用' : '不可用',
          showDivider: true
        })
        RowItem({
          title: '回答来源',
          subtitle: '这一页只负责提问文本',
          value: '桌面端 / 外部模型',
          showDivider: false
        })
      }
      .width('100%')
    }
  }

  @Builder
  usageCard() {
    Surface({ tone: 'alt', inset: Space.md }) {
      Column({ space: Space.xs }) {
        SectionTitle({ title: '推荐流程' })
        Text('说一句话 → 转写结果落进输入框 → 复制，粘到你常用的模型客户端；也可以回桌面端用浮窗直接提问。')
          .fontSize(Type.label)
          .lineHeight(Leading.body)
          .fontColor(Palette.text2())
          .width('100%')
      }
      .width('100%')
    }
  }
}
