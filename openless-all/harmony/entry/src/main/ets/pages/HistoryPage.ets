import { LengthMetrics } from '@kit.ArkUI';
import { HistoryDateFilter } from '../components/history/HistoryDateFilter';
import { HistoryDetail } from '../components/history/HistoryDetail';
import { HistoryNoteComposer } from '../components/history/HistoryNoteComposer';
import { HistoryPreviewCard } from '../components/history/HistoryPreviewCard';
import { common } from '@kit.AbilityKit';
import { AudioPlayback } from '../common/AudioPlayback';
import { Draft } from '../common/Draft';
import { Store } from '../common/Store';
import { Layout, Leading, Palette, Radius, Space, Type } from '../common/Theme';
import {
  EmptyHint,
  ErrorNote,
  Group,
  PageBackBar,
  PageScaffold,
  PrimaryButton,
  RowItem,
  PinnedSearchBar,
  SheetTitle,
  Skeleton,
  TextButton,
  notify
} from '../components/Kit';
import { Api } from '../data/Api';
import { HistoryEntry, SettingsSnapshot, StylePack, UserPreferences } from '../data/Types';

/** Fixed width of the list column on lg/xl; the detail takes the rest. */
const LIST_WIDTH: number = 360;

/** The two modes this destination now carries: transcripts and the scratchpad. */

/** Same 250 ms the iOS `.task(id: query)` search drawer waits before filtering. */
const SEARCH_DEBOUNCE_MS: number = 250;

/** Type-ahead draft save: short enough to survive a swipe away, long enough to batch. */
const DRAFT_DEBOUNCE_MS: number = 400;

const SHEET_WIDTH_WIDE: number = 420;
const STYLE_SHEET_WIDTH_WIDE: number = 540;
const MENU_MORE: string = 'more';
const MENU_CLEAR: string = 'clear';
const DAY_MS: number = 24 * 60 * 60 * 1000;

function pad2(value: number): string {
  return value < 10 ? '0' + value.toString() : value.toString();
}

/** iOS `entry.date?.formatted(date: .abbreviated, time: .shortened)`. */
function formattedDate(iso: string): string {
  if (iso.length === 0) return '时间未知';
  const date: Date = new Date(iso);
  if (Number.isNaN(date.getTime())) return '时间未知';
  return date.getFullYear().toString() + '年' + (date.getMonth() + 1).toString() + '月' +
    date.getDate().toString() + '日 ' + pad2(date.getHours()) + ':' + pad2(date.getMinutes());
}

/** iOS `HistoryFormatting.duration`: 42秒 / 1分05秒 / 1小时02分. */
function durationText(ms: number | null | undefined): string {
  if (ms === undefined || ms === null || ms <= 0) return '';
  const seconds: number = Math.floor(ms / 1000);
  if (seconds >= 3600) {
    return Math.floor(seconds / 3600).toString() + '小时' +
      Math.floor((seconds % 3600) / 60).toString() + '分';
  }
  if (seconds >= 60) return Math.floor(seconds / 60).toString() + '分' + (seconds % 60).toString() + '秒';
  return seconds.toString() + '秒';
}

/** DatePicker value: 2026-11-23. */
function dayLabel(date: Date): string {
  return date.getFullYear().toString() + '-' + pad2(date.getMonth() + 1) + '-' + pad2(date.getDate());
}

/**
 * 历史 — a 1:1 port of `ios/App/HistoryView.swift` onto the native Harmony UI.
 *
 * The iOS screen is: an inline 历史 navigation title, an ellipsis toolbar menu
 * (刷新 / 清空历史 with a destructive confirmation), a search drawer, a date-filter
 * line, a `LazyVStack` of `productCard()` previews and a detail sheet. The port
 * keeps that exact structure; only the search drawer becomes an inline
 * `SearchField` (debounced local filtering, no new backend) and the detail sheet
 * becomes the narrow-window page replacement / wide-window right column, both
 * rendering the same `HistoryDetail` component.
 *
 * 速记 stays as the Harmony-only second segment of the same screen, painted with
 * the same card vocabulary.
 *
 * `detailId` in AppStorage remains the single source of truth for the open entry,
 * so system back clears it first (see Index.onBackPress / Nav.back).
 */
@Component
export struct HistoryPage {
  @StorageLink('bp') bp: string = 'md';
  @StorageLink('phoneLayout') phoneLayout: boolean = true;
  @StorageLink('detailId') @Watch('onDetailChange') detailId: string = '';
  @StorageLink('backendRevision') @Watch('onRevision') revision: number = 0;
  @StorageLink('dictPhase') phase: string = 'idle';
  @StorageLink('dictError') dictError: string = '';

  @State private tab: number = 0;
  @State private entries: HistoryEntry[] = [];
  @State private styles: StylePack[] = [];
  @State private query: string = '';
  @State private appliedQuery: string = '';
  @State private dateFilter: boolean = false;
  @State private startDate: Date = new Date();
  @State private endDate: Date = new Date();
  @State private menuOpen: boolean = false;
  @State private menuMode: string = MENU_MORE;
  @State private styleOpen: boolean = false;
  @State private deleteOpen: boolean = false;
  @State private deleteId: string = '';
  @State private busy: boolean = false;
  @State private error: string = '';
  @State private detailError: string = '';
  @State private loading: boolean = true;
  @State private prefs: UserPreferences = {};
  @State private playing: boolean = false;
  @State private repolished: string = '';
  @State private repolishedStyle: string = '';
  @State private note: string = '';
  @State private noteError: string = '';
  @State private noteBusy: boolean = false;
  private audio: AudioPlayback = new AudioPlayback();
  private draftTimer: number = -1;
  private searchTimer: number = -1;
  private pageActive: boolean = false;
  private detailGeneration: number = 0;
  private playbackGeneration: number = 0;

  aboutToAppear(): void {
    this.pageActive = true;
    const now: Date = new Date();
    this.endDate = now;
    this.startDate = new Date(now.getFullYear(), now.getMonth(), now.getDate());
    this.reload();
    this.restoreDraft();
  }

  aboutToDisappear(): void {
    this.pageActive = false;
    this.invalidateDetailActions();
    this.flushDraft();
    this.cancelSearch();
  }

  /** Another page changed the backend (settings, credentials); pick it up. */
  private onRevision(): void {
    this.reload();
  }

  /** A different record was opened: stale polish output must not follow it. */
  private onDetailChange(): void {
    this.invalidateDetailActions();
    this.styleOpen = false;
    this.deleteOpen = false;
    this.repolished = '';
    this.repolishedStyle = '';
    this.detailError = '';
  }

  /** Selection changes invalidate UI results, not already-confirmed Core work. */
  private invalidateDetailActions(): void {
    this.detailGeneration++;
    this.playbackGeneration++;
    this.playing = false;
    this.busy = false;
    this.audio.stop();
  }

  private ownsDetail(id: string, generation: number): boolean {
    return this.pageActive && this.detailId === id && this.detailGeneration === generation;
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
    try {
      this.styles = await Api.stylePacks();
    } catch (error) {
      this.styles = [];
    }
  }

  /** True while a record replaces the list (compact windows only). */
  private detailOpen(): boolean {
    return !this.isWide() && this.tab === 0 && this.activeEntry() !== undefined;
  }

  private isWide(): boolean {
    // Portrait tablets present the phone UI, so they stay single-column too.
    return !this.phoneLayout && (this.bp === Layout.LG || this.bp === Layout.XL);
  }

  // ── Search (inline stand-in for the iOS searchable drawer) ──────────────────

  private onQuery(value: string): void {
    this.query = value;
    this.cancelSearch();
    this.searchTimer = setTimeout(() => {
      this.searchTimer = -1;
      this.appliedQuery = this.query;
    }, SEARCH_DEBOUNCE_MS);
  }

  private cancelSearch(): void {
    if (this.searchTimer === -1) return;
    clearTimeout(this.searchTimer);
    this.searchTimer = -1;
  }

  // ── Date filter (iOS start-of-day..next-day window) ────────────────────────

  private rangeStart(): number {
    const start: number = new Date(this.startDate.getFullYear(), this.startDate.getMonth(),
      this.startDate.getDate()).getTime();
    const end: number = new Date(this.endDate.getFullYear(), this.endDate.getMonth(),
      this.endDate.getDate()).getTime();
    return Math.min(start, end);
  }

  private rangeEnd(): number {
    const start: number = new Date(this.startDate.getFullYear(), this.startDate.getMonth(),
      this.startDate.getDate()).getTime();
    const end: number = new Date(this.endDate.getFullYear(), this.endDate.getMonth(),
      this.endDate.getDate()).getTime();
    return Math.max(start, end) + DAY_MS;
  }

  private pickDate(start: boolean): void {
    this.getUIContext().showDatePickerDialog({
      start: new Date(2000, 0, 1),
      end: new Date(2100, 11, 31),
      selected: start ? this.startDate : this.endDate,
      onDateAccept: (value: Date) => {
        if (start) {
          this.startDate = value;
        } else {
          this.endDate = value;
        }
      }
    });
  }

  // ── Local filtering ────────────────────────────────────────────────────────

  private filteredEntries(): HistoryEntry[] {
    const needle: string = this.appliedQuery.trim().toLowerCase();
    const from: number = this.rangeStart();
    const to: number = this.rangeEnd();
    const result: HistoryEntry[] = [];
    for (let index = 0; index < this.entries.length; index++) {
      const entry: HistoryEntry = this.entries[index];
      if (this.dateFilter) {
        const created: number = new Date(entry.createdAt).getTime();
        if (isNaN(created) || created < from || created >= to) continue;
      }
      if (needle.length > 0) {
        const text: string = (entry.finalText + ' ' + (entry.rawTranscript ?? '') + ' ' +
          (entry.asrTranscript ?? '') + ' ' + (entry.appName ?? '')).toLowerCase();
        if (text.indexOf(needle) < 0) continue;
      }
      result.push(entry);
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

  // ── Entry formatting, straight from HistoryModels.swift ────────────────────

  private hasError(entry: HistoryEntry): boolean {
    const code: string | null | undefined = entry.errorCode;
    return code !== undefined && code !== null;
  }

  /** iOS `entry.displayText`. */
  private displayText(entry: HistoryEntry): string {
    if (entry.finalText.length > 0) return entry.finalText;
    return entry.rawTranscript ?? '';
  }

  /** iOS `entry.originalText` = `asrTranscript ?? rawTranscript`. */
  private originalText(entry: HistoryEntry): string {
    const asr: string | null | undefined = entry.asrTranscript;
    if (asr !== undefined && asr !== null && asr.length > 0) return asr;
    return entry.rawTranscript ?? '';
  }

  /** iOS `entry.statusTitle`. */
  private statusTitle(entry: HistoryEntry): string {
    const code: string = entry.errorCode ?? '';
    if (code.length === 0) return '已完成';
    if (code === 'recording') return '未完成录音';
    if (code === 'cancelled') return '已取消';
    if (code === 'transcribeFailed') return '识别未完成';
    if (code === 'emptyTranscript') return '未识别到文字';
    if (code === 'polishFailed') return '润色未完成，原文已保留';
    if (code === 'insertFailed') return '文字已保留，输入未完成';
    return '未完成';
  }

  private hasAudio(entry: HistoryEntry): boolean {
    return entry.hasAudioRecording === true;
  }

  private canPolish(): boolean {
    const provider: string = this.prefs.activeLlmProvider ?? '';
    return provider.length > 0;
  }

  // ── Detail props (the component itself stays presentational) ────────────────

  private detailStatus(): string {
    const entry = this.activeEntry();
    return entry === undefined ? '' : this.statusTitle(entry);
  }

  private detailDate(): string {
    const entry = this.activeEntry();
    return entry === undefined ? '' : formattedDate(entry.createdAt);
  }

  private detailDuration(): string {
    const entry = this.activeEntry();
    return entry === undefined ? '' : durationText(entry.durationMs);
  }

  private detailFinal(): string {
    const entry = this.activeEntry();
    return entry === undefined ? '' : entry.finalText;
  }

  private detailOriginal(): string {
    const entry = this.activeEntry();
    return entry === undefined ? '' : this.originalText(entry);
  }

  private detailHasAudio(): boolean {
    const entry = this.activeEntry();
    return entry === undefined ? false : this.hasAudio(entry);
  }

  private detailShowRepolish(): boolean {
    const entry = this.activeEntry();
    if (entry === undefined) return false;
    return (entry.rawTranscript ?? '').length > 0;
  }

  // ── Actions ────────────────────────────────────────────────────────────────

  private async copy(text: string, done: string): Promise<void> {
    if (text.length === 0) return;
    try {
      await Api.copyText(text);
      notify(done);
    } catch (error) {
      this.detailError = (error as Error).message;
    }
  }

  private copyFinal(): void {
    const entry = this.activeEntry();
    if (entry === undefined) return;
    this.copy(entry.finalText, '已复制处理结果');
  }

  private copyOriginal(): void {
    const entry = this.activeEntry();
    if (entry === undefined) return;
    this.copy(this.originalText(entry), '已复制识别原文');
  }

  private copyRepolished(): void {
    this.copy(this.repolished, '已复制润色结果');
  }

  private async refresh(): Promise<void> {
    this.menuOpen = false;
    await this.reload();
  }

  /** iOS ellipsis menu「清空历史」→ destructive alert → clearLibraryHistory. */
  private async clearAll(): Promise<void> {
    if (this.busy || !this.pageActive) return;
    this.busy = true;
    this.error = '';
    try {
      await this.audio.stop();
      this.playing = false;
      await Api.clearHistory();
      if (this.detailId.length > 0) Store.put<string>('detailId', '');
      this.menuOpen = false;
      await this.reload();
      notify('已清空历史');
    } catch (error) {
      this.error = (error as Error).message;
      this.menuOpen = false;
    } finally {
      this.busy = false;
    }
  }

  private async remove(id: string): Promise<void> {
    if (this.busy || !this.pageActive || id.length === 0) return;
    this.busy = true;
    try {
      await this.audio.stop();
      this.playing = false;
      await Api.deleteHistoryEntry(id);
      if (this.detailId === id) Store.put<string>('detailId', '');
      await this.reload();
      notify('已删除');
    } catch (error) {
      this.detailError = (error as Error).message;
    } finally {
      this.busy = false;
    }
  }

  private async confirmDelete(): Promise<void> {
    await this.remove(this.deleteId);
    this.deleteOpen = false;
  }

  private async togglePlayback(entry: HistoryEntry): Promise<void> {
    if (!this.pageActive || this.detailId !== entry.id) return;
    const detailGeneration: number = this.detailGeneration;
    const playbackGeneration: number = ++this.playbackGeneration;
    if (this.playing) {
      await this.audio.stop();
      if (playbackGeneration === this.playbackGeneration) this.playing = false;
      return;
    }
    this.detailError = '';
    try {
      const payload = await Api.readRecording(entry.id);
      if (!this.ownsDetail(entry.id, detailGeneration) ||
        playbackGeneration !== this.playbackGeneration) return;
      const context = this.getUIContext().getHostContext() as common.UIAbilityContext;
      await this.audio.play(context.cacheDir, entry.id, payload.base64, (playing: boolean) => {
        if (this.ownsDetail(entry.id, detailGeneration) &&
          playbackGeneration === this.playbackGeneration) this.playing = playing;
      });
    } catch (error) {
      if (!this.ownsDetail(entry.id, detailGeneration) ||
        playbackGeneration !== this.playbackGeneration) return;
      this.playing = false;
      this.detailError = (error as Error).message;
    }
  }

  private async repolish(entry: HistoryEntry, style: StylePack): Promise<void> {
    if (this.busy || !this.pageActive || this.detailId !== entry.id) return;
    const generation: number = this.detailGeneration;
    this.styleOpen = false;
    this.busy = true;
    this.detailError = '';
    this.repolished = '';
    this.repolishedStyle = style.name;
    try {
      const raw: string = entry.rawTranscript ?? '';
      const source: string = raw.length > 0 ? raw : entry.finalText;
      const result: string = await Api.repolish(source, style.id);
      if (!this.ownsDetail(entry.id, generation)) return;
      this.repolished = result;
      notify('已重新润色');
    } catch (error) {
      if (this.ownsDetail(entry.id, generation)) this.detailError = (error as Error).message;
    } finally {
      if (this.ownsDetail(entry.id, generation)) this.busy = false;
    }
  }

  private async retranscribe(entry: HistoryEntry): Promise<void> {
    if (this.busy || !this.pageActive || this.detailId !== entry.id) return;
    const generation: number = this.detailGeneration;
    this.busy = true;
    this.detailError = '';
    try {
      await Api.retranscribe(entry.id);
      if (!this.ownsDetail(entry.id, generation)) return;
      await this.reload();
      if (this.ownsDetail(entry.id, generation)) notify('已重新转写');
    } catch (error) {
      if (this.ownsDetail(entry.id, generation)) this.detailError = (error as Error).message;
    } finally {
      if (this.ownsDetail(entry.id, generation)) this.busy = false;
    }
  }

  private openStyleMenu(): void {
    if (this.busy || !this.canPolish()) return;
    this.styleOpen = true;
  }

  private applyStyle(style: StylePack): void {
    const entry = this.activeEntry();
    if (entry === undefined) return;
    this.repolish(entry, style);
  }

  private openDelete(): void {
    if (this.busy) return;
    const entry = this.activeEntry();
    if (entry === undefined) return;
    this.deleteId = entry.id;
    this.deleteOpen = true;
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

  // ── Build ──────────────────────────────────────────────────────────────────

  build() {
    PageScaffold({
      title: this.detailOpen() ? '记录详情' : '历史',
      actionLabel: this.detailOpen() ? '完成' : (this.tab === 0 ? '历史管理' : ''),
      actionSymbol: this.detailOpen() ? '' : (this.tab === 0 ? 'more' : ''),
      scrollable: false,
      onAction: () => {
        if (this.detailOpen()) {
          Store.put<string>('detailId', '');
          return;
        }
        this.menuMode = MENU_MORE;
        this.menuOpen = true;
      }
    }) {
      if (this.tab === 0) {
        this.recordsPane()
      } else {
        this.quickNotePane()
      }
      this.sheetHost()
    }
  }

  @Builder
  recordsPane() {
    if (this.isWide()) {
      this.filterArea()
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
      this.errorNote()
      this.detailComponent()
    } else {
      this.filterArea()
      this.errorNote()
      this.listPane()
    }
  }

  /** Search first (the iOS drawer sits above the list), then the date filter line. */
  @Builder
  filterArea() {
    Column({ space: Space.sm }) {
      PinnedSearchBar({
        placeholder: '搜索历史',
        value: this.query,
        inset: false,
        showHairline: false,
        onInput: (value: string) => {
          this.onQuery(value);
        }
      })
      HistoryDateFilter({
        filterOn: this.dateFilter,
        startLabel: dayLabel(this.startDate),
        endLabel: dayLabel(this.endDate),
        onToggle: () => {
          this.dateFilter = !this.dateFilter;
        },
        onReset: () => {
          this.dateFilter = false;
        },
        onPickStart: () => {
          this.pickDate(true);
        },
        onPickEnd: () => {
          this.pickDate(false);
        }
      })
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
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
    } else if (this.entries.length === 0) {
      EmptyHint({
        symbol: 'history',
        title: '还没有识别记录',
        hint: '主动录音后，文字会保存在这里。\n历史保留在本机，不随词典同步上传。'
      })
    } else if (this.filteredEntries().length === 0) {
      Row() {
        Text('没有匹配的记录')
          .fontSize(Type.body)
          .lineHeight(Leading.body)
          .fontColor(Palette.text2())
          .textAlign(TextAlign.Center)
          .layoutWeight(1)
      }
      .width('100%')
      .padding({ top: Space.xl, bottom: Space.xl })
    } else {
      List({ space: Space.sm }) {
        ForEach(this.filteredEntries(), (entry: HistoryEntry) => {
          ListItem() {
            HistoryPreviewCard({
              timestamp: formattedDate(entry.createdAt),
              duration: durationText(entry.durationMs),
              text: this.displayText(entry),
              status: this.statusTitle(entry),
              failed: this.hasError(entry),
              selected: this.detailId === entry.id,
              onSelect: () => {
                this.audio.stop();
                this.playing = false;
                Store.put<string>('detailId', entry.id);
              }
            })
          }
        }, (entry: HistoryEntry) => entry.id)
      }
      .width('100%')
      .layoutWeight(1)
      .scrollBar(BarState.Off)
      .edgeEffect(EdgeEffect.Spring)
      .cachedCount(6)
      .contentEndOffset(Layout.dockClearance())
    }
  }

  /** Wide right column: the open record, or a hint until one is picked. */
  @Builder
  detailSlot() {
    if (this.activeEntry() === undefined) {
      Column() {
        EmptyHint({
          symbol: 'history',
          title: '选一条记录',
          hint: '左侧列表里点开任意一条'
        })
      }
      .width('100%')
      .layoutWeight(1)
      .justifyContent(FlexAlign.Center)
    } else {
      this.detailComponent()
    }
  }

  /** One detail component for both presentations (narrow replacement / wide column). */
  @Builder
  detailComponent() {
    HistoryDetail({
      status: this.detailStatus(),
      date: this.detailDate(),
      duration: this.detailDuration(),
      finalText: this.detailFinal(),
      originalText: this.detailOriginal(),
      hasAudio: this.detailHasAudio(),
      playing: this.playing,
      busy: this.busy,
      canPolish: this.canPolish(),
      showRepolish: this.detailShowRepolish(),
      styleLabel: this.repolishedStyle,
      repolished: this.repolished,
      failure: this.detailError,
      onPlay: () => {
        const entry = this.activeEntry();
        if (entry !== undefined) this.togglePlayback(entry);
      },
      onCopyFinal: () => {
        this.copyFinal();
      },
      onCopyOriginal: () => {
        this.copyOriginal();
      },
      onCopyRepolished: () => {
        this.copyRepolished();
      },
      onRetranscribe: () => {
        const entry = this.activeEntry();
        if (entry !== undefined) this.retranscribe(entry);
      },
      onPickStyle: () => {
        this.openStyleMenu();
      },
      onDelete: () => {
        this.openDelete();
      }
    })
  }

  @Builder
  quickNotePane() {
    Row() {
      TextButton({
        label: '返回记录',
        symbol: 'chevron_left',
        tone: 'quiet',
        onTap: () => {
          this.flushDraft();
          this.tab = 0;
        }
      })
      Blank()
    }
    .width('100%')

    Scroll() {
      Column({ space: Space.md }) {
        HistoryNoteComposer({
          text: this.note,
          hint: this.noteHint(),
          phase: this.phase,
          statusText: this.noteStatusText(),
          canDictate: this.canDictate(),
          onInput: (value: string) => {
            this.onNoteInput(value);
          },
          onDictate: () => {
            this.dictate();
          },
          onCopy: () => {
            this.copyNote();
          },
          onClear: () => {
            this.clearNote();
          }
        })
      }
      .width('100%')
      .alignItems(HorizontalAlign.Start)
      .padding({ bottom: Layout.dockClearance() })
    }
    .width('100%')
    .layoutWeight(1)
    .align(Alignment.Top)
    .scrollBar(BarState.Off)
    .edgeEffect(EdgeEffect.Spring)
  }

  // ── Modal layer: one anchor per concern, never two sheets stacked ───────────

  @Builder
  sheetHost() {
    Row()
      .width(0)
      .height(0)
      .bindSheet($$this.menuOpen, this.menuSheet, {
        preferType: this.isWide() ? SheetType.CENTER : SheetType.BOTTOM,
        width: this.isWide() ? SHEET_WIDTH_WIDE : '100%',
        height: this.isWide() ? SheetSize.FIT_CONTENT : SheetSize.MEDIUM,
        dragBar: true,
        radius: LengthMetrics.vp(Radius.sheet),
        showClose: true,
        onDisappear: () => {
          this.menuOpen = false;
          this.menuMode = MENU_MORE;
        }
      })

    Row()
      .width(0)
      .height(0)
      .bindSheet($$this.styleOpen, this.styleSheet, {
        preferType: this.isWide() ? SheetType.CENTER : SheetType.BOTTOM,
        width: this.isWide() ? STYLE_SHEET_WIDTH_WIDE : '100%',
        height: this.isWide() ? SheetSize.LARGE : SheetSize.MEDIUM,
        dragBar: true,
        radius: LengthMetrics.vp(Radius.sheet),
        showClose: true,
        onDisappear: () => {
          this.styleOpen = false;
        }
      })

    Row()
      .width(0)
      .height(0)
      .bindSheet($$this.deleteOpen, this.deleteSheet, {
        preferType: this.isWide() ? SheetType.CENTER : SheetType.BOTTOM,
        width: this.isWide() ? SHEET_WIDTH_WIDE : '100%',
        height: this.isWide() ? SheetSize.FIT_CONTENT : SheetSize.MEDIUM,
        dragBar: true,
        radius: LengthMetrics.vp(Radius.sheet),
        showClose: true,
        onDisappear: () => {
          this.deleteOpen = false;
        }
      })
  }

  @Builder
  menuSheet() {
    Column({ space: Space.md }) {
      if (this.menuMode === MENU_MORE) {
        RowItem({
          symbol: 'edit',
          title: '速记',
          subtitle: '草稿只留在本机，不进云同步',
          showChevron: true,
          showDivider: false,
          onTap: () => {
            this.menuOpen = false;
            this.tab = 1;
          }
        })
      }
      if (this.menuMode === MENU_CLEAR) {
        SheetTitle({
          title: '清空本机识别历史及其录音？',
          hint: '词典和独立活动统计不会被清空。此操作不可撤销。'
        })
        PrimaryButton({
          label: this.busy ? '正在清空…' : '清空历史',
          destructive: true,
          usable: !this.busy,
          onTap: () => {
            this.clearAll();
          }
        })
        TextButton({
          label: '取消',
          onTap: () => {
            this.menuOpen = false;
          }
        })
      } else {
        SheetTitle({ title: '历史管理' })
        Group() {
          Column() {
            RowItem({
              symbol: 'refresh',
              title: '刷新',
              subtitle: this.loading ? '正在读取历史…' : '',
              showDivider: true,
              muted: this.loading,
              onTap: () => {
                this.refresh();
              }
            })
            RowItem({
              symbol: 'trash',
              title: '清空历史',
              subtitle: '删除本机全部识别记录及其录音',
              showDivider: false,
              muted: this.entries.length === 0 || this.busy,
              onTap: () => {
                if (this.entries.length === 0 || this.busy) return;
                this.menuMode = MENU_CLEAR;
              }
            })
          }
          .width('100%')
        }
        TextButton({
          label: '取消',
          tone: 'quiet',
          onTap: () => {
            this.menuOpen = false;
          }
        })
      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
    .padding({ left: Space.md, right: Space.md, top: Space.sm, bottom: Space.xl })
  }

  @Builder
  styleSheet() {
    Scroll() {
    Column({ space: Space.md }) {
      SheetTitle({ title: '重新润色', hint: '选择风格后立即重新润色这条记录' })
      if (this.styles.length === 0) {
        EmptyHint({
          symbol: 'style',
          title: '还没有可用的风格',
          hint: '在「风格」里准备好文字模型和风格包后再试'
        })
      } else {
        Group() {
          Column() {
            ForEach(this.styles, (style: StylePack, index: number) => {
              RowItem({
                title: style.name,
                subtitle: style.description ?? '',
                value: this.repolishedStyle === style.name ? '已用在下方' : '',
                showDivider: index < this.styles.length - 1,
                onTap: () => {
                  this.applyStyle(style);
                }
              })
            }, (style: StylePack) => style.id)
          }
          .width('100%')
        }
      }
      TextButton({
        label: '取消',
        onTap: () => {
          this.styleOpen = false;
        }
      })
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
    .padding({ left: Space.md, right: Space.md, top: Space.sm, bottom: Space.xl })
    }
    .width('100%')
    .scrollBar(BarState.Off)
  }

  @Builder
  deleteSheet() {
    Column({ space: Space.md }) {
      SheetTitle({ title: '删除这条记录及其录音？', hint: '只删除所选本机记录，此操作不可撤销。' })
      PrimaryButton({
        label: this.busy ? '正在删除…' : '删除',
        destructive: true,
        usable: !this.busy,
        onTap: () => {
          this.confirmDelete();
        }
      })
      TextButton({
        label: '取消',
        onTap: () => {
          this.deleteOpen = false;
        }
      })
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
    .padding(Space.md)
  }
}
