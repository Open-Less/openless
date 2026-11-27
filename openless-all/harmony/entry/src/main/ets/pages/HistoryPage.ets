import { common } from '@kit.AbilityKit';
import { AudioPlayback } from '../common/AudioPlayback';
import { Draft } from '../common/Draft';
import { Fmt } from '../common/Format';
import { Store } from '../common/Store';
import { Layout, Leading, Palette, Space, Type } from '../common/Theme';
import {
  EmptyHint,
  ErrorNote,
  Group,
  InputField,
  PageBackBar,
  PageScaffold,
  PrimaryButton,
  RowItem,
  SearchField,
  SectionTitle,
  Segmented,
  Skeleton,
  Surface,
  Tag,
  TextButton,
  TonalButton,
  notify
} from '../components/Kit';
import { Api } from '../data/Api';
import { HistoryEntry, SettingsSnapshot, UserPreferences } from '../data/Types';

interface ModeLabel {
  id: string;
  label: string;
}

const MODE_LABELS: ModeLabel[] = [
  { id: 'raw', label: '原文' },
  { id: 'polish', label: '润色' },
  { id: 'translate', label: '翻译' },
  { id: 'structured', label: '结构化' }
];

/** Fixed width of the list column on lg/xl; the detail takes the rest. */
const LIST_WIDTH: number = 360;

/** The two modes this destination now carries: transcripts and the scratchpad. */
const TAB_LABELS: string[] = ['记录', '速记'];

/** Type-ahead draft save: short enough to survive a swipe away, long enough to batch. */
const DRAFT_DEBOUNCE_MS: number = 400;

function modeLabel(mode: string | undefined): string {
  if (mode === undefined || mode.length === 0) return '';
  for (let index = 0; index < MODE_LABELS.length; index++) {
    if (MODE_LABELS[index].id === mode) return MODE_LABELS[index].label;
  }
  return mode;
}

/**
 * 历史记录 v2: the transcript list and 速记 (quick note) behind one segmented
 * control, because both are "what I said / what I want to keep".
 *
 * Records: `detailId` in AppStorage is the single source of truth for the open
 * entry, so one screen covers three cases without a half-built Navigation stack:
 *   sm/md  list, or the detail pushed over it with a back bar;
 *   lg/xl  list column on the left + detail on the right (placeholder when none).
 * The system back gesture clears `detailId` first (see Index.onBackPress).
 *
 * 速记: a scratchpad with a voice-append button. The text is restored from
 * `common/Draft` (local preferences, `home_draft` / `text`), saved 400 ms after
 * the last keystroke and dropped when the note is cleared.
 */
@Component
export struct HistoryPage {
  @StorageLink('bp') bp: string = 'md';
  @StorageLink('detailId') detailId: string = '';
  @StorageLink('backendRevision') @Watch('onRevision') revision: number = 0;
  @StorageLink('dictPhase') phase: string = 'idle';
  @StorageLink('dictError') dictError: string = '';

  @State private tab: number = 0;
  @State private entries: HistoryEntry[] = [];
  @State private query: string = '';
  @State private busy: boolean = false;
  @State private error: string = '';
  @State private loading: boolean = true;
  @State private prefs: UserPreferences = {};
  @State private playing: boolean = false;
  @State private note: string = '';
  @State private noteError: string = '';
  @State private noteBusy: boolean = false;
  private audio: AudioPlayback = new AudioPlayback();
  private draftTimer: number = -1;

  aboutToAppear(): void {
    this.reload();
    this.restoreDraft();
  }

  aboutToDisappear(): void {
    this.audio.stop();
    this.flushDraft();
  }

  /** Another page changed the backend (settings, credentials); pick it up. */
  private onRevision(): void {
    this.reload();
  }

  private async reload(): Promise<void> {
    this.loading = true;
    try {
      this.entries = await Api.history();
      const settings: SettingsSnapshot = await Api.settings();
      this.prefs = settings.preferences;
      this.error = '';
    } catch (error) {
      this.error = (error as Error).message;
    } finally {
      this.loading = false;
    }
  }

  private isWide(): boolean {
    return this.bp === Layout.LG || this.bp === Layout.XL;
  }

  private filteredEntries(): HistoryEntry[] {
    if (this.query.length === 0) return this.entries;
    const needle: string = this.query.toLowerCase();
    const result: HistoryEntry[] = [];
    for (let index = 0; index < this.entries.length; index++) {
      const entry: HistoryEntry = this.entries[index];
      const text: string = (entry.finalText + ' ' + (entry.rawTranscript ?? '') + ' ' +
        (entry.appName ?? '')).toLowerCase();
      if (text.indexOf(needle) >= 0) result.push(entry);
    }
    return result;
  }

  private activeEntry(): HistoryEntry | undefined {
    if (this.detailId.length === 0) return undefined;
    for (let index = 0; index < this.entries.length; index++) {
      if (this.entries[index].id === this.detailId) return this.entries[index];
    }
    return undefined;
  }

  private subtitleText(): string {
    if (this.query.length > 0) {
      return '筛出 ' + this.filteredEntries().length.toString() + ' / ' +
        this.entries.length.toString() + ' 条';
    }
    return this.entries.length.toString() + ' 条记录 · 只保存在这台设备上';
  }

  private previewOf(entry: HistoryEntry): string {
    const text: string = Fmt.preview(entry.finalText, this.isWide() ? 46 : 60);
    return text.length > 0 ? text : '（无转写文本）';
  }

  /** Second line of a list row: time · mode · app · failure. */
  private metaLine(entry: HistoryEntry): string {
    let line: string = Fmt.relative(entry.createdAt);
    const mode: string = modeLabel(entry.mode);
    if (mode.length > 0) line = line + ' · ' + mode;
    const app: string = entry.appName ?? '';
    if (app.length > 0) line = line + ' · ' + app;
    if (this.hasError(entry)) line = line + ' · 失败';
    return line;
  }

  private hasError(entry: HistoryEntry): boolean {
    return entry.errorCode !== undefined && entry.errorCode !== null;
  }

  private appLabel(entry: HistoryEntry): string {
    const app: string = entry.appName ?? '';
    return app.length > 0 ? app : '—';
  }

  private insertLabel(entry: HistoryEntry): string {
    const status: string = entry.insertStatus ?? '';
    return status.length > 0 ? status : '—';
  }

  private errorLabel(entry: HistoryEntry): string {
    const code: string = entry.errorCode ?? '';
    return code.length > 0 ? code : '—';
  }

  private asrText(entry: HistoryEntry): string {
    const asr: string = entry.asrTranscript ?? '';
    return asr;
  }

  /** Raw ASR/whisper text used by the "复制原文" action. */
  private copySource(entry: HistoryEntry): string {
    const asr: string = this.asrText(entry);
    if (asr.length > 0) return asr;
    return entry.rawTranscript ?? '';
  }

  private showRawAction(entry: HistoryEntry): boolean {
    const raw: string = entry.rawTranscript ?? '';
    return raw.length > 0 && raw !== entry.finalText;
  }

  private hasAudio(entry: HistoryEntry): boolean {
    return entry.hasAudioRecording === true;
  }

  private latency(provider: string | null | undefined, ms: number | null | undefined): string {
    const name: string = provider === undefined || provider === null || provider.length === 0
      ? '—' : provider;
    if (ms === undefined || ms === null || ms <= 0) return name;
    return name + ' · ' + ms.toString() + ' ms';
  }

  private async copy(text: string, done: string): Promise<void> {
    try {
      await Api.copyText(text);
      notify(done);
    } catch (error) {
      this.error = (error as Error).message;
    }
  }

  private async remove(id: string): Promise<void> {
    this.busy = true;
    try {
      await Api.deleteHistoryEntry(id);
      if (this.detailId === id) Store.put<string>('detailId', '');
      await this.reload();
      notify('已删除');
    } catch (error) {
      this.error = (error as Error).message;
    } finally {
      this.busy = false;
    }
  }

  private async togglePlayback(entry: HistoryEntry): Promise<void> {
    if (this.playing) {
      await this.audio.stop();
      this.playing = false;
      return;
    }
    this.error = '';
    try {
      const payload = await Api.readRecording(entry.id);
      const context = this.getUIContext().getHostContext() as common.UIAbilityContext;
      await this.audio.play(context.cacheDir, entry.id, payload.base64, (playing: boolean) => {
        this.playing = playing;
      });
    } catch (error) {
      this.playing = false;
      this.error = '播放失败：' + (error as Error).message;
    }
  }

  private async repolish(entry: HistoryEntry): Promise<void> {
    this.busy = true;
    this.error = '';
    try {
      const source: string = entry.rawTranscript !== undefined && entry.rawTranscript.length > 0
        ? entry.rawTranscript : entry.finalText;
      const text: string = await Api.repolish(source, this.prefs.activeStylePackId ?? '');
      await Api.copyText(text);
      await this.reload();
      notify('已重新润色并复制');
    } catch (error) {
      this.error = (error as Error).message;
    } finally {
      this.busy = false;
    }
  }

  private async retranscribe(id: string): Promise<void> {
    this.busy = true;
    this.error = '';
    try {
      await Api.retranscribe(id);
      await this.reload();
      notify('已重新转写');
    } catch (error) {
      this.error = (error as Error).message;
    } finally {
      this.busy = false;
    }
  }

  // ── 速记 (quick note) ──────────────────────────────────────────────────────
  private async dictate(): Promise<void> {
    if (this.noteBusy) return;
    this.noteBusy = true;
    this.noteError = '';
    try {
      if (this.phase === 'idle' || this.dictError.length > 0) {
        await Api.startDictation();
      } else {
        const voice = await Api.stopDictation();
        const text: string = voice.polishedText.length > 0 ? voice.polishedText : voice.rawText;
        if (text.length > 0) {
          this.note = this.note.length > 0 ? this.note + '\n' + text : text;
          this.scheduleDraftSave();
          notify('已追加一段');
        }
      }
    } catch (error) {
      this.noteError = (error as Error).message;
    } finally {
      this.noteBusy = false;
    }
  }

  private onNoteInput(value: string): void {
    this.note = value;
    this.scheduleDraftSave();
  }

  private copyNote(): void {
    if (this.note.length === 0) return;
    this.copy(this.note, '已复制全部内容');
  }

  private clearNote(): void {
    if (this.note.length === 0) return;
    this.note = '';
    this.cancelDraftSave();
    const context: common.UIAbilityContext | undefined = this.draftContext();
    if (context !== undefined) Draft.clear(context);
    notify('已清空草稿');
  }

  private noteHint(): string {
    if (this.phase === 'recording') return '正在录音，点一下结束并追加';
    if (this.phase === 'starting') return '正在准备麦克风…';
    if (this.phase === 'processing') return '正在转写，稍后追加到末尾';
    return '语音会追加到末尾 · 草稿自动保存';
  }

  /** The note only owns its own error; a leftover dictError is shown when idle. */
  private noteStatusText(): string {
    if (this.noteError.length > 0) return this.noteError;
    if (this.phase === 'idle') return this.dictError;
    return '';
  }

  private canDictate(): boolean {
    return !this.noteBusy && this.phase !== 'processing';
  }

  private draftContext(): common.UIAbilityContext | undefined {
    try {
      const context = this.getUIContext().getHostContext();
      if (context === undefined) return undefined;
      return context as common.UIAbilityContext;
    } catch (error) {
      return undefined;
    }
  }

  private async restoreDraft(): Promise<void> {
    const context: common.UIAbilityContext | undefined = this.draftContext();
    if (context === undefined) return;
    try {
      const stored: string = await Draft.load(context);
      // A keystroke that beat the disk wins: never overwrite a fresh edit.
      if (stored.length > 0 && this.note.length === 0) this.note = stored;
    } catch (error) {
      console.warn('[OpenLess History] draft restore failed');
    }
  }

  private scheduleDraftSave(): void {
    this.cancelDraftSave();
    this.draftTimer = setTimeout(() => {
      this.draftTimer = -1;
      this.persistDraft();
    }, DRAFT_DEBOUNCE_MS);
  }

  private cancelDraftSave(): void {
    if (this.draftTimer === -1) return;
    clearTimeout(this.draftTimer);
    this.draftTimer = -1;
  }

  /** Leaving or switching tabs writes a pending keystroke immediately. */
  private flushDraft(): void {
    if (this.draftTimer === -1) return;
    this.cancelDraftSave();
    this.persistDraft();
  }

  private persistDraft(): void {
    const context: common.UIAbilityContext | undefined = this.draftContext();
    if (context === undefined) return;
    if (this.note.length === 0) {
      Draft.clear(context);
      return;
    }
    Draft.save(context, this.note);
  }

  build() {
    PageScaffold({
      title: this.pageTitle(),
      subtitle: this.pageSubtitle(),
      actionLabel: this.pageActionLabel(),
      actionSymbol: this.pageActionSymbol(),
      scrollable: false,
      onAction: () => {
        this.reload();
      }
    }) {
      this.tabBar()
      if (this.tab === 0) {
        this.recordsPane()
      } else {
        this.quickNotePane()
      }
    }
  }

  private pageTitle(): string {
    return this.tab === 0 ? '历史' : '速记';
  }

  private pageSubtitle(): string {
    if (this.tab === 1) return '想到什么先记下来，稍后再整理';
    return this.subtitleText();
  }

  private pageActionLabel(): string {
    return this.tab === 0 ? '刷新' : '';
  }

  private pageActionSymbol(): string {
    return this.tab === 0 ? 'refresh' : '';
  }

  @Builder
  tabBar() {
    Segmented({
      items: TAB_LABELS,
      selected: this.tab,
      onSelect: (index: number) => {
        if (index === this.tab) return;
        this.tab = index;
        if (index === 0) this.flushDraft();
      }
    })
  }

  @Builder
  recordsPane() {
    if (this.isWide()) {
      this.searchRow()
      this.errorNote()
      Row({ space: Space.md }) {
        Column() {
          this.listPane()
        }
        .width(LIST_WIDTH)
        .height('100%')

        Column() {
          this.detailSlot()
        }
        .layoutWeight(1)
        .height('100%')
      }
      .width('100%')
      .layoutWeight(1)
      .alignItems(VerticalAlign.Top)
    } else if (this.activeEntry() !== undefined) {
      Row() {
        PageBackBar({
          label: '历史',
          onTap: () => {
            Store.put<string>('detailId', '');
          }
        })
      }
      .width('100%')

      this.errorNote()
      this.detailPane(this.activeEntry() as HistoryEntry)
    } else {
      this.searchRow()
      this.errorNote()
      this.listPane()
    }
  }

  @Builder
  quickNotePane() {
    Scroll() {
      Column({ space: Space.md }) {
        if (this.noteStatusText().length > 0) {
          ErrorNote({
            message: this.noteStatusText(),
            onRetry: () => {
              this.dictate();
            }
          })
        }
        if (this.isWide()) {
          Row({ space: Space.md }) {
            Column({ space: Space.md }) {
              this.noteActionCard()
              this.noteEditorCard()
            }
            .layoutWeight(5)
            .alignItems(HorizontalAlign.Start)

            Column({ space: Space.md }) {
              this.noteTipsCard()
            }
            .layoutWeight(4)
            .alignItems(HorizontalAlign.Start)
          }
          .width('100%')
          .alignItems(VerticalAlign.Top)
        } else {
          Column({ space: Space.md }) {
            this.noteActionCard()
            this.noteEditorCard()
            this.noteTipsCard()
          }
          .width('100%')
          .alignItems(HorizontalAlign.Start)
        }
      }
      .width('100%')
      .alignItems(HorizontalAlign.Start)
      .padding({ bottom: Space.xl })
    }
    .width('100%')
    .layoutWeight(1)
    .align(Alignment.Top)
    .scrollBar(BarState.Off)
    .edgeEffect(EdgeEffect.Spring)
  }

  @Builder
  noteActionCard() {
    Surface({ inset: Space.md }) {
      Column({ space: Space.sm }) {
        SectionTitle({ title: '草稿', hint: this.noteHint() })

        Flex({ wrap: FlexWrap.Wrap, justifyContent: FlexAlign.Start, alignItems: ItemAlign.Center }) {
          Row() {
            PrimaryButton({
              label: this.phase === 'recording' ? '结束并追加' : '语音追加',
              usable: this.canDictate(),
              onTap: () => {
                this.dictate();
              }
            })
          }
          .margin({ right: Space.xs, bottom: Space.xs })

          Row() {
            TonalButton({
              label: '复制全部',
              symbol: 'copy',
              usable: this.note.length > 0,
              onTap: () => {
                this.copyNote();
              }
            })
          }
          .margin({ right: Space.xs, bottom: Space.xs })

          Row() {
            TextButton({
              label: '清空',
              symbol: 'trash',
              tone: 'danger',
              usable: this.note.length > 0,
              onTap: () => {
                this.clearNote();
              }
            })
          }
          .margin({ right: Space.xs, bottom: Space.xs })
        }
        .width('100%')

        Row({ space: Space.xs }) {
          Tag({ text: this.note.length.toString() + ' 字', tone: 'neutral' })
          if (this.phase === 'recording') {
            Tag({ text: '正在录音', tone: 'danger' })
          } else if (this.phase === 'processing') {
            Tag({ text: '转写中', tone: 'accent' })
          }
        }
        .width('100%')
      }
      .width('100%')
      .alignItems(HorizontalAlign.Start)
    }
  }

  @Builder
  noteEditorCard() {
    Surface({ inset: Space.md }) {
      InputField({
        label: '内容',
        placeholder: '在这里写，或用上面的语音追加一段',
        value: this.note,
        multiline: true,
        onInput: (value: string) => {
          this.onNoteInput(value);
        }
      })
    }
  }

  @Builder
  noteTipsCard() {
    Surface({ tone: 'alt', inset: Space.md }) {
      Column({ space: Space.xs }) {
        SectionTitle({ title: '说明' })
        Text('草稿会自动保存在这台设备上，离开页面再回来可以接着写；点「清空」会连同保存的草稿一起删除。语音追加复用听写链路，转写结果接在最后一段之后。')
          .fontSize(Type.label)
          .lineHeight(Leading.body)
          .fontColor(Palette.text2())
          .width('100%')
      }
      .width('100%')
    }
  }

  @Builder
  searchRow() {
    SearchField({
      placeholder: '搜索转写内容或应用名',
      value: this.query,
      onInput: (value: string) => {
        this.query = value;
      }
    })
  }

  @Builder
  errorNote() {
    if (this.error.length > 0) {
      ErrorNote({
        message: this.error,
        onRetry: () => {
          this.reload();
        }
      })
    }
  }

  @Builder
  listPane() {
    if (this.loading && this.entries.length === 0) {
      Skeleton({ rows: 4 })
    } else if (this.filteredEntries().length === 0) {
      Group() {
        EmptyHint({
          symbol: 'history',
          title: this.query.length > 0 ? '没有匹配的记录' : '还没有听写记录',
          hint: this.query.length > 0 ? '换个关键词试试' : '在概览页点麦克风说第一句话'
        })
      }
    } else {
      List({ space: Space.xs }) {
        ForEach(this.filteredEntries(), (entry: HistoryEntry) => {
          ListItem() {
            Group() {
              this.entryRow(entry)
            }
          }
        }, (entry: HistoryEntry) => entry.id)
      }
      .width('100%')
      .layoutWeight(1)
      .scrollBar(BarState.Off)
      .edgeEffect(EdgeEffect.Spring)
      .cachedCount(6)
    }
  }

  @Builder
  entryRow(entry: HistoryEntry) {
    RowItem({
      symbol: this.detailId === entry.id ? 'check' : 'history',
      accentSymbol: this.detailId === entry.id,
      title: this.previewOf(entry),
      subtitle: this.metaLine(entry),
      value: entry.durationMs !== undefined && entry.durationMs !== null && entry.durationMs > 0
        ? Fmt.duration(entry.durationMs) : '',
      showChevron: !this.isWide(),
      showDivider: false,
      onTap: () => {
        Store.put<string>('detailId', entry.id);
      }
    })
  }

  /** Wide right column: the open record, or a hint until one is picked. */
  @Builder
  detailSlot() {
    if (this.activeEntry() === undefined) {
      Column() {
        Group() {
          EmptyHint({
            symbol: 'history',
            title: '选一条记录',
            hint: '左侧列表里点开任意一条'
          })
        }
      }
      .width('100%')
      .layoutWeight(1)
      .justifyContent(FlexAlign.Center)
    } else {
      this.detailPane(this.activeEntry() as HistoryEntry)
    }
  }

  @Builder
  detailPane(entry: HistoryEntry) {
    Scroll() {
      Column({ space: Space.md }) {
        this.transcriptCard(entry)
        this.asrCard(entry)
        this.paramsCard(entry)
        this.actionRow(entry)
      }
      .width('100%')
      .alignItems(HorizontalAlign.Start)
      .padding({ bottom: Space.xl })
    }
    .width('100%')
    .layoutWeight(1)
    .align(Alignment.Top)
    .scrollBar(BarState.Off)
    .edgeEffect(EdgeEffect.Spring)
  }

  @Builder
  transcriptCard(entry: HistoryEntry) {
    Surface({ inset: Space.md }) {
      Column({ space: Space.sm }) {
        Row({ space: Space.sm }) {
          Text(Fmt.relative(entry.createdAt))
            .fontSize(Type.label)
            .lineHeight(Leading.label)
            .fontColor(Palette.text2())
            .layoutWeight(1)
          if (this.hasError(entry)) {
            Tag({ text: '失败', tone: 'danger' })
          }
        }
        .width('100%')

        Text(entry.finalText)
          .fontSize(Type.body)
          .lineHeight(Leading.body)
          .fontColor(Palette.text1())
          .width('100%')
          .copyOption(CopyOptions.LocalDevice)

        Flex({ wrap: FlexWrap.Wrap, justifyContent: FlexAlign.Start }) {
          if (modeLabel(entry.mode).length > 0) {
            Row() {
              Tag({ text: modeLabel(entry.mode), tone: 'accent' })
            }
            .margin({ right: Space.xs, bottom: Space.xxs })
          }
          if ((entry.appName ?? '').length > 0) {
            Row() {
              Tag({ text: this.appLabel(entry) })
            }
            .margin({ right: Space.xs, bottom: Space.xxs })
          }
          if (entry.durationMs !== undefined && entry.durationMs !== null && entry.durationMs > 0) {
            Row() {
              Tag({ text: Fmt.duration(entry.durationMs) })
            }
            .margin({ right: Space.xs, bottom: Space.xxs })
          }
          if (this.hasAudio(entry)) {
            Row() {
              Tag({ text: '有录音', tone: 'success' })
            }
            .margin({ right: Space.xs, bottom: Space.xxs })
          }
        }
        .width('100%')
      }
      .width('100%')
      .alignItems(HorizontalAlign.Start)
    }
  }

  @Builder
  asrCard(entry: HistoryEntry) {
    if (this.asrText(entry).length > 0 && this.asrText(entry) !== entry.finalText) {
      Surface({ inset: Space.md }) {
        Column({ space: Space.sm }) {
          SectionTitle({ title: '识别原文', hint: '润色之前的 ASR 输出，用来区分听错还是改写' })
          Text(this.asrText(entry))
            .fontSize(Type.label)
            .lineHeight(Leading.label)
            .fontColor(Palette.text2())
            .width('100%')
            .copyOption(CopyOptions.LocalDevice)
        }
        .width('100%')
        .alignItems(HorizontalAlign.Start)
      }
    }
  }

  @Builder
  paramsCard(entry: HistoryEntry) {
    Group() {
      Column() {
        Row() {
          SectionTitle({ title: '参数' })
        }
        .width('100%')
        .padding({ left: Space.md, right: Space.md, top: Space.sm, bottom: Space.xxs })

        RowItem({
          title: '模式',
          value: modeLabel(entry.mode).length > 0 ? modeLabel(entry.mode) : '—',
          showChevron: false,
          showDivider: true
        })
        RowItem({
          title: '时长',
          value: Fmt.duration(entry.durationMs),
          showChevron: false,
          showDivider: true
        })
        RowItem({
          title: '转写',
          value: this.latency(entry.asrProvider, entry.asrMs),
          showChevron: false,
          showDivider: true
        })
        RowItem({
          title: '润色',
          value: this.latency(entry.llmProvider, entry.polishMs),
          showChevron: false,
          showDivider: true
        })
        RowItem({
          title: '应用',
          value: this.appLabel(entry),
          showChevron: false,
          showDivider: true
        })
        RowItem({
          title: '写入',
          value: this.insertLabel(entry),
          showChevron: false,
          showDivider: !this.hasError(entry)
        })
        if (this.hasError(entry)) {
          RowItem({
            title: '错误',
            value: this.errorLabel(entry),
            showChevron: false,
            showDivider: false
          })
        }
      }
      .width('100%')
    }
  }

  @Builder
  actionRow(entry: HistoryEntry) {
    Flex({ wrap: FlexWrap.Wrap, justifyContent: FlexAlign.Start, alignItems: ItemAlign.Center }) {
      if (this.hasAudio(entry)) {
        Row() {
          PrimaryButton({
            label: this.playing ? '停止播放' : '播放录音',
            onTap: () => {
              this.togglePlayback(entry);
            }
          })
        }
        .margin({ right: Space.xs, bottom: Space.xs })

        Row() {
          TonalButton({
            label: this.busy ? '重新转写中…' : '重新转写',
            symbol: 'refresh',
            usable: !this.busy,
            onTap: () => {
              this.retranscribe(entry.id);
            }
          })
        }
        .margin({ right: Space.xs, bottom: Space.xs })
      }

      Row() {
        TonalButton({
          label: this.busy ? '重润色中…' : '重新润色',
          symbol: 'sparkles',
          usable: !this.busy,
          onTap: () => {
            this.repolish(entry);
          }
        })
      }
      .margin({ right: Space.xs, bottom: Space.xs })

      Row() {
        TonalButton({
          label: '复制全文',
          symbol: 'copy',
          onTap: () => {
            this.copy(entry.finalText, '已复制全文');
          }
        })
      }
      .margin({ right: Space.xs, bottom: Space.xs })

      if (this.showRawAction(entry)) {
        Row() {
          TextButton({
            label: '复制原文',
            symbol: 'copy',
            onTap: () => {
              this.copy(this.copySource(entry), '已复制原文');
            }
          })
        }
        .margin({ right: Space.xs, bottom: Space.xs })
      }

      Row() {
        TextButton({
          label: '删除',
          symbol: 'trash',
          tone: 'danger',
          usable: !this.busy,
          onTap: () => {
            this.remove(entry.id);
          }
        })
      }
      .margin({ right: Space.xs, bottom: Space.xs })
    }
    .width('100%')
  }
}
