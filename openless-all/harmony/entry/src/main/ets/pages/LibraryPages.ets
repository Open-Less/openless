import {
  CorrectionRuleRow,
  StyleSummaryCard,
  VocabularyRow
} from '../components/library/LibraryPresentation';
import { Fmt } from '../common/Format';
import { symbolOf } from '../components/Icons';
import { LengthMetrics } from '@kit.ArkUI';
import { Layout, Leading, Palette, Radius, Size, Space, Type, floatShadow } from '../common/Theme';
import {
  Banner,
  AccentTextButton,
  CapsuleChip,
  CapsuleTrack,
  EmptyHint,
  ErrorNote,
  Group,
  InputField,
  PageScaffold,
  PrimaryButton,
  RowItem,
  SearchField,
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
import { Api, PendingCorrection, VocabPreset } from '../data/Api';
import { CorrectionRule, DictionaryEntry, MarketplacePack, SettingsSnapshot, StylePack } from '../data/Types';
import { StyleEditing } from '../data/StyleEditing';

/**
 * Library surfaces: 词典 / 纠正规则 / 风格（风格包 + 风格市场同页）.
 *
 * Ported from `ios/App/VocabularyView.swift` and `ios/App/StyleLibraryView.swift`
 * rather than re-designed: the same inline page title with one trailing 44pt
 * button, the same search field + segmented filter above the list, the same
 * phrase-first dictionary rows and the same style cards (`StyleSummary` next to
 * a 36pt sparkles tile, one trailing capsule, a 使用中 badge).
 *
 * The only structural adaptation is Harmony-specific and pre-existing: wide
 * windows keep a right detail column and compact windows lift the same detail
 * body into a `bindSheet`, so one sheet anchor per page switches content through
 * `sheetMode` and two sheets can never stack. Every mutation still goes through
 * the typed `Api` façade — no entry, rule or pack lost a button.
 */

const SHEET_ADD: string = 'add';
const SHEET_DETAIL: string = 'detail';
const SHEET_PRESETS: string = 'presets';
const SHEET_CREATE: string = 'create';
const SHEET_EDIT: string = 'edit';
const SHEET_CONFIRM: string = 'confirm';
const SHEET_SUGGEST: string = 'suggest';
const SHEET_MARKET: string = 'market';

/**
 * iOS `VocabularySource.filters` — `.all`, `.learned`, `.manual`.
 * Core classifies an entry by its note marker, exactly like
 * `CoreVocabularyEntry.classifiedSource` (`shared_types::LEARNED_VOCAB_NOTE`).
 */
const VOCAB_SOURCES: string[] = ['所有', '自动添加', '手动添加'];

/** iOS `sourceFilters`: All has no glyph; learned uses sparkles, manual uses a book. */
const VOCAB_CHIPS: CapsuleChip[] = [
  { label: '所有', symbol: '', tint: 'neutral' },
  { label: '自动添加', symbol: 'sparkles', tint: 'teal' },
  { label: '手动添加', symbol: 'vocab', tint: 'neutral' }
];
const LEARNED_VOCAB_NOTE: string = '从手改中自动收集';

const LIBRARY_FILTERS: string[] = ['全部', '已启用', '已停用'];
const MARKET_SORTS: string[] = ['按热度', '最新'];

/**
 * 词典：`VocabularyView` 的本机词条列表 + 增删启停 + 只读预设词组。
 *
 * `xl`/`lg` 时左侧列表右侧详情；窄屏点行把同一份详情抬进半模态。
 */
@Component
export struct VocabPage {
  @StorageLink('bp') bp: string = 'md';
  @StorageLink('phoneLayout') phoneLayout: boolean = true;
  @StorageLink('keyboardVisible') keyboardVisible: boolean = false;

  @State private entries: DictionaryEntry[] = [];
  @State private presets: VocabPreset[] = [];
  @State private query: string = '';
  /** iOS reveals the search field from the floating orb instead of pinning it. */
  @State private searchOpen: boolean = false;
  private searchFocusPending: boolean = false;
  /** Index into `VOCAB_SOURCES` (iOS `VocabularySource.filters`). */
  @State private source: number = 0;
  @State private error: string = '';
  @State private loading: boolean = false;
  @State private busy: boolean = false;
  @State private selectedId: string = '';
  @State private sheetOpen: boolean = false;
  @State private sheetMode: string = SHEET_ADD;
  @State private draft: string = '';
  @State private editDraft: string = '';
  @State private editingPhrase: boolean = false;

  aboutToAppear(): void {
    this.reload();
  }

  private isWide(): boolean {
    // Portrait tablets present the phone UI, so they stay single-column too.
    return !this.phoneLayout && (this.bp === Layout.LG || this.bp === Layout.XL);
  }

  private async reload(): Promise<void> {
    this.loading = true;
    try {
      this.entries = await Api.vocabulary();
      try {
        this.presets = await Api.vocabularyPresets();
      } catch (error) {
        this.presets = [];
      }
      this.error = '';
    } catch (error) {
      this.error = (error as Error).message;
    } finally {
      this.loading = false;
    }
  }

  /** iOS `CoreVocabularyEntry.classifiedSource`, with Core's note-marker fallback. */
  private sourceOf(entry: DictionaryEntry): string {
    const note: string | null | undefined = entry.note;
    if (note !== undefined && note !== null && note === LEARNED_VOCAB_NOTE) return 'learned';
    return 'manual';
  }

  private filtered(): DictionaryEntry[] {
    const needle: string = this.query.trim().toLowerCase();
    const result: DictionaryEntry[] = [];
    for (let index = 0; index < this.entries.length; index++) {
      const entry: DictionaryEntry = this.entries[index];
      const origin: string = this.sourceOf(entry);
      if (this.source === 1 && origin !== 'learned') continue;
      if (this.source === 2 && origin !== 'manual') continue;
      if (needle.length > 0 && entry.phrase.toLowerCase().indexOf(needle) < 0) continue;
      result.push(entry);
    }
    return result;
  }

  /** `-1` when the selection is empty or the entry disappeared after a reload. */
  private detailIndex(): number {
    for (let index = 0; index < this.entries.length; index++) {
      if (this.entries[index].id === this.selectedId) return index;
    }
    return -1;
  }

  private detailPhrase(): string {
    const index: number = this.detailIndex();
    return index < 0 ? '' : this.entries[index].phrase;
  }

  private detailEnabled(): boolean {
    const index: number = this.detailIndex();
    return index >= 0 && this.entries[index].enabled === true;
  }

  private detailHits(): string {
    const index: number = this.detailIndex();
    if (index < 0) return '';
    return (this.entries[index].hits ?? 0).toString() + ' 次命中';
  }

  private detailNote(): string {
    const index: number = this.detailIndex();
    if (index < 0) return '';
    const note: string | null | undefined = this.entries[index].note;
    if (note === undefined || note === null) return '';
    return note;
  }

  private detailSince(): string {
    const index: number = this.detailIndex();
    if (index < 0) return '';
    return Fmt.relative(this.entries[index].createdAt ?? '');
  }

  private detailSource(): string {
    const index: number = this.detailIndex();
    if (index < 0) return '';
    return this.sourceOf(this.entries[index]) === 'learned' ? '自动添加' : '手动添加';
  }

  private openSheet(mode: string): void {
    this.sheetMode = mode;
    this.sheetOpen = true;
  }

  private closeSheet(): void {
    this.sheetOpen = false;
  }

  private selectEntry(id: string): void {
    this.selectedId = id;
    this.editingPhrase = false;
    this.editDraft = '';
    if (!this.isWide()) this.openSheet(SHEET_DETAIL);
  }

  private startEditPhrase(): void {
    const current: string = this.detailPhrase();
    if (current.length === 0) return;
    this.editDraft = current;
    this.editingPhrase = true;
  }

  private async saveEditedPhrase(): Promise<void> {
    const id: string = this.selectedId;
    const next: string = this.editDraft.trim();
    if (id.length === 0 || next.length === 0 || this.busy) return;
    this.busy = true;
    try {
      await Api.updateVocabulary(id, next);
      this.editingPhrase = false;
      this.editDraft = '';
      notify('已更新词条');
      await this.reload();
    } catch (error) {
      const message: string = (error as Error).message;
      this.error = message;
      notify('更新失败：' + message);
    } finally {
      this.busy = false;
    }
  }

  private async add(): Promise<void> {
    const phrase: string = this.draft.trim();
    if (phrase.length === 0) return;
    this.busy = true;
    try {
      await Api.addVocabulary(phrase);
      this.draft = '';
      this.closeSheet();
      notify('已添加「' + phrase + '」');
      await this.reload();
    } catch (error) {
      const message: string = (error as Error).message;
      this.error = message;
      notify('添加失败：' + message);
    } finally {
      this.busy = false;
    }
  }

  private async setEnabled(enabled: boolean): Promise<void> {
    const id: string = this.selectedId;
    if (id.length === 0) return;
    try {
      await Api.setVocabularyEnabled(id, enabled);
      notify(enabled ? '已启用该词条' : '已停用该词条');
      await this.reload();
    } catch (error) {
      this.error = (error as Error).message;
      notify('操作失败：' + this.error);
    }
  }

  private async removeSelected(): Promise<void> {
    const id: string = this.selectedId;
    if (id.length === 0) return;
    try {
      await Api.removeVocabulary(id);
      this.selectedId = '';
      this.closeSheet();
      notify('已删除词条');
      await this.reload();
    } catch (error) {
      this.error = (error as Error).message;
      notify('删除失败：' + this.error);
    }
  }

  build() {
    // iOS `VocabularyView` (frame-verified): the 34pt title is drawn INSIDE the
    // scrolling body (`.toolbar(.hidden)`, `navigationTitle("")`), the source chips
    // scroll horizontally, and two 52pt orbs float above the dock.
    Stack({ alignContent: Alignment.Bottom }) {
      Column() {
      Scroll() {
        Column({ space: Space.lg }) {
          Text('词典')
            .fontSize(Type.pageTitle)
            .lineHeight(Leading.pageTitle)
            .fontWeight(FontWeight.Bold)
            .fontColor(Palette.text1())
            .width('100%')
            .accessibilityLevel('yes')

          CapsuleTrack({
            chips: VOCAB_CHIPS,
            selected: this.source,
            onSelect: (index: number) => {
              if (index === this.source) return;
              this.source = index;
            }
          })

          if (this.error.length > 0) {
            ErrorNote({
              message: '词典暂时无法更新，请重试。',
              onRetry: () => {
                this.reload();
              }
            })
          }

          if (this.isWide()) {
            this.wideBody()
          } else {
            this.narrowBody()
          }
        }
        .width('100%')
        .padding({ top: Space.lg, bottom: Layout.dockClearance() + 76 })
      }
      .layoutWeight(1)
      .width('100%')
      .align(Alignment.TopStart)
      .scrollBar(BarState.Off)
      .edgeEffect(EdgeEffect.Spring)

      }
      .width('100%')
      .height('100%')
      this.bottomActions()
      this.sheetHost()
    }
    .width('100%')
    .height('100%')
    .padding({ left: Layout.gutter(this.bp), right: Layout.gutter(this.bp) })
  }

  /**
   * iOS `dictionaryActions` (VocabularyView.swift:97-122): 52pt search orb that
   * expands into a field of width `window - 64`, plus a 52pt add orb, 24pt side
   * padding and 12pt vertical padding, all above the dock.
   */
  @Builder
  bottomActions() {
    Row({ space: Space.sm }) {
      if (this.searchOpen) {
        Row({ space: Space.xs }) {
          SymbolGlyph(symbolOf('search'))
            .fontSize(Size.iconSmall)
            .fontColor([Palette.text3()])
          TextInput({ placeholder: '搜索词条', text: this.query })
            .id('vocab-search-input')
            .enableKeyboardOnFocus(true)
            // Request only once after the conditional input is visible, not
            // before mount or with a guessing timer (Huawei focus FAQ 483).
            // https://developer.huawei.com/consumer/cn/doc/doccenter-dev-faq/faqs-arkui-483
            .onVisibleAreaChange([1.0], (visible: boolean, ratio: number) => {
              if (!visible || ratio < 1 || !this.searchOpen || !this.searchFocusPending) return;
              this.searchFocusPending = false;
              try { this.getUIContext().getFocusController().requestFocus('vocab-search-input'); }
              catch (error) { console.warn('[OpenLess Vocabulary] search focus unavailable'); }
            })
            .fontSize(Type.body)
            .fontColor(Palette.text1())
            .placeholderColor(Palette.text3())
            .backgroundColor(Color.Transparent)
            .padding(0)
            .layoutWeight(1)
            .onChange((text: string) => {
              this.query = text;
            })
          Button({ type: ButtonType.Circle }) {
            SymbolGlyph(symbolOf('close'))
              .fontSize(Size.iconSmall)
              .fontColor([Palette.text3()])
          }
          .width(32)
          .height(32)
          .backgroundColor(Color.Transparent)
          .id('vocab-search-close')
          .accessibilityText('关闭搜索')
          .onClick(() => {
            this.query = '';
            this.searchFocusPending = false;
            this.searchOpen = false;
          })
        }
        .layoutWeight(1)
        .height(52)
        .padding({ left: Space.md, right: Space.md })
        .backgroundColor(Palette.card())
        .borderRadius(Radius.pill)
        .shadow(floatShadow())
      } else {
        this.orb('search', '搜索词条', () => {
          this.searchFocusPending = true;
          this.searchOpen = true;
        })
        Blank()
      }
      this.orb('add', '添加词条', () => {
        this.draft = '';
        this.openSheet(SHEET_ADD);
      })
    }
    .width('100%')
    .padding({ left: Space.xxs, right: Space.xxs, top: Space.sm, bottom: Space.sm })
    .margin({ bottom: this.keyboardVisible ? 0 : Layout.dockClearance() })
  }

  @Builder
  orb(symbol: string, label: string, action: () => void) {
    Button({ type: ButtonType.Circle }) {
      SymbolGlyph(symbolOf(symbol))
        .fontSize(Type.section)
        .fontColor([Palette.text1()])
    }
    .id('vocab-action-' + symbol)
    .width(52)
    .height(52)
    .backgroundColor(Palette.card())
    .shadow(floatShadow())
    .hoverEffect(HoverEffect.Highlight)
    .accessibilityText(label)
    .onClick(action)
  }

  @Builder
  narrowBody() {
    Column({ space: Space.xxs }) {
      if (this.entries.length === 0) {
        EmptyHint({
          symbol: 'vocab',
          title: this.loading ? '正在读取词典…' : '还没有词条',
          hint: '点右下角加号，添加常用人名或术语。'
        })
      } else if (this.filtered().length === 0) {
        EmptyHint({ symbol: 'vocab', title: '没有匹配的词条', hint: '试试其他来源或搜索词。' })
      } else {
        ForEach(this.filtered(), (entry: DictionaryEntry) => {
          VocabularyRow({
            phrase: entry.phrase,
            isEnabled: entry.enabled === true,
            selected: this.selectedId === entry.id,
            onSelect: () => {
              this.selectEntry(entry.id);
            }
          })
        }, (entry: DictionaryEntry) => entry.id)
        Text('点按词条可编辑、停用、删除或查看详情。')
          .fontSize(Type.caption)
          .lineHeight(Leading.caption)
          .fontColor(Palette.text2())
          .width('100%')
          .padding({ top: Space.xs })
      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
  }

  @Builder
  wideBody() {
    Row({ space: Space.md }) {
      Column() {
        this.listBody()
      }
      .width(360)
      .layoutWeight(0)

      if (this.selectedId.length > 0) {
        Column() {
          this.detailBody(true)
        }
        .layoutWeight(1)
      } else {
        Column() {
          EmptyHint({ symbol: 'vocab', title: '选一个词条', hint: '左侧列表里点开任意一条' })
        }
        .layoutWeight(1)
        .justifyContent(FlexAlign.Start)
      }
    }
    .width('100%')
    .alignItems(VerticalAlign.Top)
  }

  /** iOS vocabulary cards: phrase first, source marker, disabled state greyed out. */
  @Builder
  listBody() {
    if (this.entries.length === 0) {
      EmptyHint({
        symbol: 'vocab',
        title: this.loading ? '正在读取词典…' : '还没有词条',
        hint: '点右下角加号，添加常用人名或术语。'
      })
    } else if (this.filtered().length === 0) {
      EmptyHint({ symbol: 'vocab', title: '没有匹配的词条', hint: '试试其他来源或搜索词。' })
    } else {
      Column({ space: Space.xxs }) {
        ForEach(this.filtered(), (entry: DictionaryEntry) => {
          VocabularyRow({
            phrase: entry.phrase,
            isEnabled: entry.enabled === true,
            selected: this.selectedId === entry.id,
            onSelect: () => {
              this.selectEntry(entry.id);
            }
          })
        }, (entry: DictionaryEntry) => entry.id)
      }
      .width('100%')
      .alignItems(HorizontalAlign.Start)
    }
  }

  /** iOS `VocabularyDetails`: 词条 / 详细信息 sections, note, enable toggle, delete. */
  @Builder
  detailBody(inline: boolean) {
    Scroll() {
      Column({ space: Space.sm }) {
        this.detailSectionTitle('词条')
        Group({ radius: 12 }) {
          Column({ space: Space.xs }) {
            if (this.editingPhrase) {
              Column({ space: Space.sm }) {
                InputField({
                  label: '修改词条',
                  placeholder: '输入专有名词或短语',
                  value: this.editDraft,
                  onInput: (value: string) => {
                    this.editDraft = value;
                  }
                })
                Row({ space: Space.sm }) {
                  PrimaryButton({
                    label: this.busy ? '保存中…' : '保存',
                    usable: !this.busy && this.editDraft.trim().length > 0,
                    onTap: () => {
                      this.saveEditedPhrase();
                    }
                  })
                  TextButton({
                    label: '取消',
                    tone: 'quiet',
                    usable: !this.busy,
                    onTap: () => {
                      this.editingPhrase = false;
                      this.editDraft = '';
                    }
                  })
                }
                .width('100%')
              }
              .width('100%')
              .padding(Space.md)
            } else {
              Row({ space: Space.sm }) {
                Text(this.detailPhrase())
                  .fontSize(Type.section)
                  .lineHeight(Leading.section)
                  .fontWeight(FontWeight.Medium)
                  .fontColor(Palette.text1())
                  .layoutWeight(1)
                  .copyOption(CopyOptions.LocalDevice)
                TextButton({
                  label: '编辑',
                  symbol: 'edit',
                  usable: !this.busy,
                  onTap: () => {
                    this.startEditPhrase();
                  }
                })
              }
              .width('100%')
              .padding(Space.md)
              .alignItems(VerticalAlign.Center)
            }
          }
          .width('100%')
          .alignItems(HorizontalAlign.Start)
        }

        this.detailSectionTitle('详细信息')
        Group({ radius: 12 }) {
          RowItem({ title: '添加方式', value: this.detailSource(), showDivider: true })
          RowItem({ title: '状态', value: this.detailEnabled() ? '已启用' : '已停用', showDivider: true })
          RowItem({ title: '使用次数', value: this.detailHits(), showDivider: true })
          RowItem({ title: '添加时间', value: this.detailSince(), showDivider: false })
        }

        if (this.detailNote().length > 0) {
          Group({ radius: 12 }) {
            Column() {
              Text(this.detailNote())
                .fontSize(Type.body)
                .lineHeight(Leading.body)
                .fontColor(Palette.text1())
                .width('100%')
                .padding(Space.md)
                .copyOption(CopyOptions.LocalDevice)
            }
            .width('100%')
            .alignItems(HorizontalAlign.Start)
          }
        }

        SettingSwitch({
          label: '参与转写替换',
          hint: '停用后仍保留词条，但不会被替换',
          checked: this.detailEnabled(),
          showDivider: false,
          onToggle: (value: boolean) => {
            this.setEnabled(value);
          }
        })

        Text('确认后的中文词条会用于键盘候选。')
          .fontSize(Type.caption)
          .lineHeight(Leading.caption)
          .fontColor(Palette.text2())
          .width('100%')
          .padding({ left: 4, top: Space.xs })

        TextButton({
          label: '删除词条',
          symbol: 'delete',
          tone: 'danger',
          usable: this.selectedId.length > 0 && !this.busy,
          onTap: () => {
            this.removeSelected();
          }
        })
        .width('100%')

        if (inline) {
          Row() {
            PrimaryButton({
              label: '完成',
              usable: true,
              onTap: () => {
                this.selectedId = '';
              }
            })
          }
          .width('100%')
          .padding({ top: Space.xs })
        }
      }
      .width('100%')
      .alignItems(HorizontalAlign.Start)
    }
    .width('100%')
    .layoutWeight(1)
    .scrollBar(BarState.Off)
  }

  /** iOS `Section("…")` header above an inset-grouped card. */
  @Builder
  detailSectionTitle(title: string) {
    Text(title)
      .fontSize(Type.caption)
      .lineHeight(Leading.caption)
      .fontColor(Palette.text2())
      .width('100%')
      .padding({ left: 4, top: Space.xs })
  }

  @Builder
  sheetHost() {
    Row()
      .width(0)
      .height(0)
      .bindSheet($$this.sheetOpen, this.sheetBody, {
        preferType: this.isWide() ? SheetType.CENTER : SheetType.BOTTOM,
        width: this.isWide() ? 560 : '100%',
        height: this.isWide() ? SheetSize.FIT_CONTENT : SheetSize.MEDIUM,
        dragBar: true,
        showClose: this.sheetMode !== SHEET_DETAIL,
        radius: LengthMetrics.vp(Radius.sheet),
        onDisappear: () => {
          this.sheetOpen = false;
        }
      })
  }

  @Builder
  sheetBody() {
    Scroll() {
      Column({ space: Space.md }) {
        if (this.sheetMode === SHEET_DETAIL) {
          SheetTitle({
            title: '词条详情',
            actionLabel: '完成',
            onAction: () => {
              this.closeSheet();
            }
          })
          .id('vocab-detail-done')
          this.detailBody(false)
        } else if (this.sheetMode === SHEET_PRESETS) {
          SheetTitle({
            title: '预设词组',
            hint: '来自加密文档库，随云同步一起更新，共 ' + this.presets.length.toString() + ' 组'
          })
          if (this.presets.length === 0) {
            EmptyHint({ symbol: 'list', title: '还没有预设词组', hint: '文档库里的词组整理好之后会自动出现在这里' })
          } else {
            Group() {
              ForEach(this.presets, (preset: VocabPreset) => {
                RowItem({
                  title: preset.name !== undefined && preset.name !== null && preset.name.length > 0
                    ? preset.name : '未命名词组',
                  subtitle: (preset.phrases ?? []).join('、'),
                  showDivider: false
                })
              }, (preset: VocabPreset, index: number) => 'preset-' + index.toString())
            }
          }
        } else {
          SheetTitle({ title: '添加词条', hint: '输入常用词或专有名词，转写时优先命中' })
          InputField({
            label: '词条',
            placeholder: '例如：OpenLess',
            value: this.draft,
            onInput: (value: string) => {
              this.draft = value;
            }
          })
          PrimaryButton({
            label: '添加',
            usable: this.draft.trim().length > 0 && !this.busy,
            onTap: () => {
              this.add();
            }
          })
          .id('vocab-add-save')
        }
      }
      .width('100%')
      .alignItems(HorizontalAlign.Start)
      .padding({ left: Space.md, right: Space.md, top: Space.lg, bottom: Space.xl })
    }
    .width('100%')
    .scrollBar(BarState.Off)
  }
}

@Component
export struct CorrectionsPage {
  @StorageLink('bp') bp: string = 'md';
  @StorageLink('phoneLayout') phoneLayout: boolean = true;

  @State private rules: CorrectionRule[] = [];
  @State private pending: PendingCorrection[] = [];
  @State private pattern: string = '';
  @State private replacement: string = '';
  @State private query: string = '';
  @State private filter: number = 0;
  @State private error: string = '';
  @State private loading: boolean = false;
  @State private busy: boolean = false;
  @State private selectedId: string = '';
  @State private sheetOpen: boolean = false;
  @State private sheetMode: string = SHEET_ADD;

  aboutToAppear(): void {
    this.reload();
  }

  private isWide(): boolean {
    // Portrait tablets present the phone UI, so they stay single-column too.
    return !this.phoneLayout && (this.bp === Layout.LG || this.bp === Layout.XL);
  }

  private async reload(): Promise<void> {
    this.loading = true;
    try {
      this.rules = await Api.correctionRules();
      try {
        this.pending = await Api.pendingCorrections();
      } catch (error) {
        this.pending = [];
      }
      this.error = '';
    } catch (error) {
      this.error = (error as Error).message;
    } finally {
      this.loading = false;
    }
  }

  private filtered(): CorrectionRule[] {
    const needle: string = this.query.trim().toLowerCase();
    const result: CorrectionRule[] = [];
    for (let index = 0; index < this.rules.length; index++) {
      const rule: CorrectionRule = this.rules[index];
      if (this.filter === 1 && rule.enabled !== true) continue;
      if (this.filter === 2 && rule.enabled === true) continue;
      if (needle.length > 0 &&
        (rule.pattern + ' ' + rule.replacement).toLowerCase().indexOf(needle) < 0) continue;
      result.push(rule);
    }
    return result;
  }

  private detailIndex(): number {
    for (let index = 0; index < this.rules.length; index++) {
      if (this.rules[index].id === this.selectedId) return index;
    }
    return -1;
  }

  private detailPattern(): string {
    const index: number = this.detailIndex();
    return index < 0 ? '' : this.rules[index].pattern;
  }

  private detailReplacement(): string {
    const index: number = this.detailIndex();
    return index < 0 ? '' : this.rules[index].replacement;
  }

  private detailEnabled(): boolean {
    const index: number = this.detailIndex();
    return index >= 0 && this.rules[index].enabled === true;
  }

  private detailLearned(): boolean {
    const index: number = this.detailIndex();
    return index >= 0 && this.rules[index].source === 'learned';
  }

  private detailSince(): string {
    const index: number = this.detailIndex();
    if (index < 0) return '';
    return Fmt.relative(this.rules[index].createdAt ?? '');
  }

  private openSheet(mode: string): void {
    this.sheetMode = mode;
    this.sheetOpen = true;
  }

  private closeSheet(): void {
    this.sheetOpen = false;
  }

  private selectRule(id: string): void {
    this.selectedId = id;
    if (!this.isWide()) this.openSheet(SHEET_DETAIL);
  }

  private async add(): Promise<void> {
    const pattern: string = this.pattern.trim();
    const replacement: string = this.replacement.trim();
    if (pattern.length === 0) return;
    this.busy = true;
    try {
      await Api.addCorrectionRule(pattern, replacement);
      this.pattern = '';
      this.replacement = '';
      this.closeSheet();
      notify('已添加纠正规则');
      await this.reload();
    } catch (error) {
      const message: string = (error as Error).message;
      this.error = message;
      notify('添加失败：' + message);
    } finally {
      this.busy = false;
    }
  }

  private async setEnabled(enabled: boolean): Promise<void> {
    const id: string = this.selectedId;
    if (id.length === 0) return;
    try {
      await Api.setCorrectionRuleEnabled(id, enabled);
      notify(enabled ? '规则已启用' : '规则已停用');
      await this.reload();
    } catch (error) {
      this.error = (error as Error).message;
      notify('操作失败：' + this.error);
    }
  }

  private async removeSelected(): Promise<void> {
    const id: string = this.selectedId;
    if (id.length === 0) return;
    try {
      await Api.removeCorrectionRule(id);
      this.selectedId = '';
      this.closeSheet();
      notify('已删除规则');
      await this.reload();
    } catch (error) {
      this.error = (error as Error).message;
      notify('删除失败：' + this.error);
    }
  }

  private async acceptSuggestion(id: string): Promise<void> {
    try {
      await Api.acceptCorrection(id);
      notify('已采纳为规则');
      await this.reload();
    } catch (error) {
      this.error = (error as Error).message;
      notify('操作失败：' + this.error);
    }
  }

  private async rejectSuggestion(id: string): Promise<void> {
    try {
      await Api.rejectCorrection(id);
      notify('已忽略该建议');
      await this.reload();
    } catch (error) {
      this.error = (error as Error).message;
      notify('操作失败：' + this.error);
    }
  }

  private async dismissAllSuggestions(): Promise<void> {
    if (this.busy || this.pending.length === 0) return;
    this.busy = true;
    try {
      await Api.dismissSuggestions();
      notify('已忽略全部建议');
      await this.reload();
    } catch (error) {
      this.error = (error as Error).message;
      notify('操作失败：' + this.error);
    } finally {
      this.busy = false;
    }
  }

  build() {
    PageScaffold({
      title: '纠正规则',
      subtitle: this.rules.length.toString() + ' 条规则 · 在润色前替换',
      actionLabel: '新建规则',
      actionSymbol: 'add',
      scrollable: false,
      gap: Space.sm,
      onAction: () => {
        this.pattern = '';
        this.replacement = '';
        this.openSheet(SHEET_ADD);
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
      this.filterRow()
      if (this.isWide()) {
        this.wideBody()
      } else {
        this.narrowBody()
      }
      this.sheetHost()
    }
  }

  @Builder
  filterRow() {
    Column({ space: Space.sm }) {
      SearchField({
        placeholder: '搜索规则，例如错字或正确写法',
        value: this.query,
        onInput: (value: string) => {
          this.query = value;
        }
      })
      Segmented({
        items: LIBRARY_FILTERS,
        selected: this.filter,
        onSelect: (index: number) => {
          this.filter = index;
        }
      })
      Row({ space: Space.sm }) {
        TonalButton({
          label: this.pending.length > 0 ? '学习建议 ' + this.pending.length.toString() : '学习建议',
          symbol: 'sparkles',
          usable: this.pending.length > 0,
          onTap: () => {
            this.openSheet(SHEET_SUGGEST);
          }
        })
      }
      .width('100%')
      .justifyContent(FlexAlign.Start)
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
  }

  @Builder
  narrowBody() {
    Scroll() {
      Column({ space: Space.sm }) {
        this.listBody()
      }
      .width('100%')
      .padding({ bottom: Layout.dockClearance() })
    }
    .width('100%')
    .layoutWeight(1)
    .align(Alignment.Top)
    .scrollBar(BarState.Off)
    .edgeEffect(EdgeEffect.Spring)
  }

  @Builder
  wideBody() {
    Row({ space: Space.md }) {
      Column() {
        Scroll() {
          Column({ space: Space.sm }) {
            this.listBody()
          }
          .width('100%')
          .padding({ bottom: Layout.dockClearance() })
        }
        .width('100%')
        .layoutWeight(1)
        .align(Alignment.Top)
        .scrollBar(BarState.Off)
        .edgeEffect(EdgeEffect.Spring)
      }
      .layoutWeight(5)
      .height('100%')

      Column() {
        if (this.detailIndex() >= 0) {
          Scroll() {
            Surface({ inset: Space.md }) {
              this.detailBody()
            }
          }
          .width('100%')
          .layoutWeight(1)
          .align(Alignment.Top)
          .scrollBar(BarState.Off)
          .edgeEffect(EdgeEffect.Spring)
        } else {
          Surface({ inset: Space.md }) {
            EmptyHint({
              symbol: 'corrections',
              title: '选一条规则',
              hint: '右侧可以启停或删除这条替换规则'
            })
          }
        }
      }
      .layoutWeight(4)
      .height('100%')
    }
    .width('100%')
    .layoutWeight(1)
    .alignItems(VerticalAlign.Top)
  }

  @Builder
  listBody() {
    if (this.loading && this.rules.length === 0) {
      Skeleton({ rows: 4 })
    } else if (this.filtered().length === 0) {
      if (this.rules.length === 0) {
        EmptyHint({
          symbol: 'corrections',
          title: '还没有规则',
          hint: '规则越具体越好，避免误伤正常文本'
        })
      } else {
        EmptyHint({
          symbol: 'search',
          title: '没有匹配的规则',
          hint: '换个关键词，或把筛选切回「全部」'
        })
      }
    } else {
      // Same row geometry as 词典: the replacement is the big line and the
      // original pattern sits above it, struck through.
      Group() {
        ForEach(this.filtered(), (rule: CorrectionRule, index: number) => {
          CorrectionRuleRow({
            pattern: rule.pattern,
            replacement: rule.replacement,
            source: rule.source === 'learned' ? 'learned' : 'manual',
            isEnabled: rule.enabled === true,
            selected: this.selectedId === rule.id,
            divider: index < this.filtered().length - 1,
            onSelect: () => {
              this.selectRule(rule.id);
            }
          })
        }, (rule: CorrectionRule) => rule.id)
      }
      Text('点按规则可查看来源、启停或删除。')
        .fontSize(Type.caption)
        .lineHeight(Leading.caption)
        .fontColor(Palette.text2())
        .width('100%')
    }
  }

  @Builder
  detailBody() {
    Column({ space: Space.sm }) {
      Column({ space: Space.xs }) {
        Text(this.detailPattern())
          .fontSize(Type.cardTitle)
          .lineHeight(Leading.cardTitle)
          .fontWeight(FontWeight.Medium)
          .fontColor(Palette.text2())
          .decoration({ type: TextDecorationType.LineThrough, color: Palette.text3() })
          .width('100%')
        Text('↓')
          .fontSize(Type.caption)
          .fontColor(Palette.text3())
        Text(this.detailReplacement())
          .fontSize(Type.cardTitle)
          .lineHeight(Leading.cardTitle)
          .fontWeight(FontWeight.Medium)
          .fontColor(Palette.text1())
          .width('100%')
      }
      .width('100%')
      .alignItems(HorizontalAlign.Start)

      Row({ space: Space.xs }) {
        if (this.detailLearned()) {
          Tag({ text: '自动学习', tone: 'accent' })
        } else {
          Tag({ text: '手动添加', tone: 'neutral' })
        }
        Tag({
          text: this.detailEnabled() ? '已启用' : '已停用',
          tone: this.detailEnabled() ? 'success' : 'neutral'
        })
      }
      .width('100%')
      .justifyContent(FlexAlign.Start)

      Text('创建于 ' + this.detailSince())
        .fontSize(Type.caption)
        .lineHeight(Leading.caption)
        .fontColor(Palette.text3())
        .width('100%')

      SettingSwitch({
        label: '启用这条规则',
        hint: '停用后润色阶段会跳过它',
        checked: this.detailEnabled(),
        showDivider: false,
        onToggle: (value: boolean) => {
          this.setEnabled(value);
        }
      })

      Row({ space: Space.sm }) {
        if (!this.isWide()) {
          PrimaryButton({
            label: '完成',
            onTap: () => {
              this.closeSheet();
            }
          })
        }
        TextButton({
          label: '删除规则',
          symbol: 'trash',
          tone: 'danger',
          onTap: () => {
            this.removeSelected();
          }
        })
      }
      .width('100%')
      .justifyContent(FlexAlign.Start)
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
  }

  @Builder
  addBody() {
    Column({ space: Space.md }) {
      SheetTitle({
        title: '新建规则',
        hint: '把「听错的样子」替换成「正确的写法」'
      })
      InputField({
        label: '错误的写法',
        placeholder: '例如「霍姆式」',
        value: this.pattern,
        onInput: (value: string) => {
          this.pattern = value;
        }
      })
      InputField({
        label: '正确的写法',
        placeholder: '例如「HarmonyOS」',
        value: this.replacement,
        onInput: (value: string) => {
          this.replacement = value;
        }
      })
      Row({ space: Space.sm }) {
        PrimaryButton({
          label: this.busy ? '保存中…' : '添加规则',
          usable: !this.busy && this.pattern.trim().length > 0,
          onTap: () => {
            this.add();
          }
        })
        TextButton({
          label: '取消',
          tone: 'quiet',
          onTap: () => {
            this.closeSheet();
          }
        })
      }
      .width('100%')
      .justifyContent(FlexAlign.End)
    }
    .width('100%')
  }

  @Builder
  suggestionsBody() {
    Scroll() {
    Column({ space: Space.md }) {
      SheetTitle({
        title: '学习建议',
        hint: '模型从历史里发现的重复纠正，采纳后成为规则'
      })
      if (this.pending.length === 0) {
        EmptyHint({
          symbol: 'sparkles',
          title: '暂时没有新建议',
          hint: '多改几处同样的错字，模型会把它提出来'
        })
      } else {
        Column({ space: Space.sm }) {
          Row() {
            TextButton({
              label: '全部忽略',
              tone: 'quiet',
              usable: !this.busy,
              onTap: () => {
                this.dismissAllSuggestions();
              }
            })
          }
          .width('100%')
          .justifyContent(FlexAlign.End)
          ForEach(this.pending, (item: PendingCorrection) => {
            Surface({ inset: Space.sm }) {
              Column({ space: Space.xs }) {
                Text(item.pattern + ' → ' + item.replacement)
                  .fontSize(Type.body)
                  .lineHeight(Leading.body)
                  .fontColor(Palette.text1())
                  .width('100%')
                Row({ space: Space.xs }) {
                  TonalButton({
                    label: '接受',
                    symbol: 'check',
                    onTap: () => {
                      this.acceptSuggestion(item.id);
                    }
                  })
                  TextButton({
                    label: '忽略',
                    tone: 'quiet',
                    onTap: () => {
                      this.rejectSuggestion(item.id);
                    }
                  })
                }
                .width('100%')
                .justifyContent(FlexAlign.Start)
              }
              .width('100%')
              .alignItems(HorizontalAlign.Start)
            }
          }, (item: PendingCorrection) => item.id)
        }
        .width('100%')
      }
    }
    .width('100%')
    }
    .width('100%')
    .scrollBar(BarState.Off)
  }

  @Builder
  sheetBody() {
    Column({ space: Space.md }) {
      if (this.sheetMode === SHEET_ADD) {
        this.addBody()
      } else if (this.sheetMode === SHEET_SUGGEST) {
        this.suggestionsBody()
      } else if (this.sheetMode === SHEET_DETAIL) {
        this.detailBody()
      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
    .padding({ left: Space.md, right: Space.md, top: Space.sm, bottom: Space.xl })
  }

  @Builder
  sheetHost() {
    Row()
      .width(0)
      .height(0)
      .bindSheet($$this.sheetOpen, this.sheetBody, {
        preferType: this.isWide() ? SheetType.CENTER : SheetType.BOTTOM,
        width: this.isWide() ? 560 : '100%',
        height: this.isWide() ? SheetSize.FIT_CONTENT : SheetSize.MEDIUM,
        dragBar: true,
        showClose: this.sheetMode !== SHEET_DETAIL,
        radius: LengthMetrics.vp(Radius.sheet),
        onDisappear: () => {
          this.sheetOpen = false;
        }
      })
  }
}

/**
 * 风格：`StyleLibraryView` 的「我的风格 / 风格市场」两段同页。
 *
 * 页标题固定为「风格」；顶部是一条钉住的搜索框 + 与 Dock 双向绑定的 `styleSection`
 * 分段（0 = 我的风格，1 = 风格市场）。卡片是 iOS `StyleCard` + `StyleSummary`：
 * 36pt 图标块、名称与「使用中」徽标、三行简介、caption 元信息，右侧一枚 capsule。
 * 两段共用一个 sheet 锚点与同一套两栏骨架，切段只按需首次加载市场。
 */
@Component
export struct StylePage {
  @StorageLink('bp') bp: string = 'md';
  @StorageLink('phoneLayout') phoneLayout: boolean = true;
  @StorageLink('backendRevision') revision: number = 0;

  /** 当前段：0 = 我的风格，1 = 风格市场。 */
  @StorageLink('styleSection') @Watch('onStyleSection') segment: number = 0;
  @State private error: string = '';

  // ── 本地风格包 ────────────────────────────────────────────────────────────
  @State private query: string = '';
  @State private filter: number = 0;
  @State private packs: StylePack[] = [];
  @State private active: string = '';
  @State private loading: boolean = false;
  @State private busy: boolean = false;
  @State private selectedId: string = '';
  @State private newName: string = '';
  @State private newPrompt: string = '';
  @State private draftPrompt: string = '';
  private editId: string = '';
  private viewActive: boolean = false;
  private readGeneration: number = 0;
  private marketGeneration: number = 0;
  private marketSearchTimer: number = -1;
  private loginGeneration: number = 0;
  private loginFlowId: string = '';
  private loginTimer: number = -1;
  private loginWake?: () => void;

  // ── 风格市场（原独立的市场页状态，整组搬入本页） ─────────────────────────
  @State private marketQuery: string = '';
  @State private marketSort: string = 'popular';
  @State private marketPacks: MarketplacePack[] = [];
  @State private marketSelectedId: string = '';
  @State private marketLoading: boolean = false;
  @State private marketLoaded: boolean = false;
  @State private signedIn: boolean = false;
  @State private loginState: string = '';
  @State private loginCode: string = '';
  @State private loginBusy: boolean = false;
  @State private likedIds: string[] = [];

  // ── 两段共用的半模态：一个锚点 + sheetMode 决定内容 ───────────────────────
  @State private sheetOpen: boolean = false;
  @State private sheetMode: string = SHEET_CREATE;

  aboutToAppear(): void {
    this.viewActive = true;
    this.reload();
    this.onStyleSection();
  }

  aboutToDisappear(): void {
    this.viewActive = false;
    this.readGeneration++;
    this.marketGeneration++;
    this.marketLoading = false;
    if (this.marketSearchTimer >= 0) clearTimeout(this.marketSearchTimer);
    this.marketSearchTimer = -1;
    this.cancelLogin();
  }

  private isWide(): boolean {
    // Portrait tablets present the phone UI, so they stay single-column too.
    return !this.phoneLayout && (this.bp === Layout.LG || this.bp === Layout.XL);
  }

  private onStyleSection(): void {
    if (this.segment !== 1) {
      this.marketGeneration++;
      this.marketLoading = false;
      if (this.marketSearchTimer >= 0) clearTimeout(this.marketSearchTimer);
      this.marketSearchTimer = -1;
      this.cancelLogin();
    } else if (!this.marketLoaded && !this.marketLoading) {
      this.reloadMarket();
    }
  }

  /** 段切换只按需首次加载市场，之后完全保留两段各自的状态。 */
  private switchSegment(index: number): void {
    this.segment = index;
    if (index === 1 && !this.marketLoaded && !this.marketLoading) {
      this.reloadMarket();
    }
  }

  private subtitleText(): string {
    if (this.segment === 1) {
      return this.marketPacks.length.toString() + ' 个社区风格包 · 需要网络与 GitHub 账号';
    }
    return this.packs.length.toString() + ' 个风格包 · 决定润色的语气';
  }

  /** iOS toolbar: `.library` 段右侧是「导入风格」的加号；市场段刷新。 */
  private actionLabel(): string {
    return this.segment === 1 ? '刷新' : '导入风格';
  }

  private actionSymbol(): string {
    return this.segment === 1 ? 'refresh' : 'add';
  }

  private triggerAction(): void {
    if (this.segment === 1) {
      this.reloadMarket();
      return;
    }
    this.importZip();
  }

  /** iOS `PinnedSearchBar` 的占位文案只有一句「搜索风格」。 */
  private searchPlaceholder(): string {
    return '搜索风格';
  }

  private searchValue(): string {
    return this.segment === 1 ? this.marketQuery : this.query;
  }

  private onSearchInput(value: string): void {
    if (this.segment === 1) {
      this.marketQuery = value;
      this.marketGeneration++;
      if (this.marketSearchTimer >= 0) clearTimeout(this.marketSearchTimer);
      this.marketSearchTimer = setTimeout(() => {
        this.marketSearchTimer = -1;
        this.reloadMarket();
      }, 300);
      return;
    }
    this.query = value;
  }

  // ── 本地风格包 ────────────────────────────────────────────────────────────

  private async reload(): Promise<void> {
    const generation: number = ++this.readGeneration;
    this.loading = true;
    try {
      const packs: StylePack[] = await Api.stylePacks();
      const snapshot: SettingsSnapshot = await Api.settings();
      if (!this.viewActive || generation !== this.readGeneration) return;
      this.packs = packs;
      this.active = snapshot.preferences.activeStylePackId ?? '';
      this.error = '';
    } catch (error) {
      if (this.viewActive && generation === this.readGeneration) this.error = (error as Error).message;
    } finally {
      if (generation === this.readGeneration) this.loading = false;
    }
  }

  private filtered(): StylePack[] {
    const needle: string = this.query.trim().toLowerCase();
    const result: StylePack[] = [];
    for (let index = 0; index < this.packs.length; index++) {
      const pack: StylePack = this.packs[index];
      if (this.filter === 1 && pack.enabled === false) continue;
      if (this.filter === 2 && pack.enabled !== false) continue;
      if (needle.length > 0 &&
        (pack.name + ' ' + (pack.description ?? '')).toLowerCase().indexOf(needle) < 0) continue;
      result.push(pack);
    }
    return result;
  }

  private detailIndex(): number {
    for (let index = 0; index < this.packs.length; index++) {
      if (this.packs[index].id === this.selectedId) return index;
    }
    return -1;
  }

  private detailName(): string {
    const index: number = this.detailIndex();
    return index < 0 ? '' : this.packs[index].name;
  }

  private detailDescription(): string {
    const index: number = this.detailIndex();
    if (index < 0) return '';
    return this.packs[index].description ?? '';
  }

  private detailPrompt(): string {
    const index: number = this.detailIndex();
    if (index < 0) return '';
    return this.packs[index].prompt;
  }

  private detailEnabled(): boolean {
    const index: number = this.detailIndex();
    return index >= 0 && this.packs[index].enabled !== false;
  }

  private detailBuiltIn(): boolean {
    const index: number = this.detailIndex();
    return index >= 0 && this.packs[index].kind === 'builtin';
  }

  private detailActive(): boolean {
    return this.selectedId.length > 0 && this.selectedId === this.active;
  }

  private openSheet(mode: string): void {
    if (this.busy) return;
    this.sheetMode = mode;
    this.sheetOpen = true;
  }

  private closeSheet(): void {
    if (this.busy) return;
    this.sheetOpen = false;
  }

  private selectPack(id: string): void {
    if (this.busy) return;
    this.selectedId = id;
    this.draftPrompt = '';
    if (!this.isWide()) this.openSheet(SHEET_DETAIL);
  }

  private startEdit(): void {
    if (this.busy || this.detailIndex() < 0) return;
    this.editId = this.selectedId;
    this.draftPrompt = this.detailPrompt();
    this.openSheet(SHEET_EDIT);
  }

  private startCreate(): void {
    if (this.busy) return;
    this.newName = '';
    this.newPrompt = '';
    this.openSheet(SHEET_CREATE);
  }

  private async createPack(): Promise<void> {
    const name: string = this.newName.trim();
    const prompt: string = this.newPrompt;
    if (name.length === 0 || this.busy) return;
    this.busy = true;
    try {
      await Api.createStylePack(name, prompt);
      if (!this.viewActive) return;
      this.newName = '';
      this.newPrompt = '';
      this.sheetOpen = false;
      notify('已创建风格「' + name + '」');
      await this.reload();
    } catch (error) {
      if (this.viewActive) {
        this.error = (error as Error).message;
        notify('创建失败：' + this.error);
      }
    } finally {
      this.busy = false;
    }
  }

  private async savePrompt(): Promise<void> {
    const id: string = this.editId;
    const prompt: string = this.draftPrompt;
    if (id.length === 0 || this.busy) return;
    this.busy = true;
    try {
      await Api.updateStylePrompt(id, prompt);
      if (!this.viewActive) return;
      if (this.editId === id && this.sheetMode === SHEET_EDIT) this.sheetOpen = false;
      notify('提示词已保存');
      await this.reload();
    } catch (error) {
      if (this.viewActive) {
        this.error = (error as Error).message;
        notify('保存失败：' + this.error);
      }
    } finally {
      this.busy = false;
    }
  }

  private async setEnabled(enabled: boolean): Promise<void> {
    const id: string = this.selectedId;
    if (id.length === 0 || this.busy) return;
    this.busy = true;
    try {
      await Api.setStylePackEnabled(id, enabled);
      if (!this.viewActive) return;
      notify(enabled ? '风格已启用' : '风格已停用');
      await this.reload();
    } catch (error) {
      if (this.viewActive) { this.error = (error as Error).message; notify('操作失败：' + this.error); }
    } finally { this.busy = false; }
  }

  private async activate(id: string): Promise<void> {
    if (id.length === 0 || id === this.active || this.busy) return;
    this.busy = true;
    try {
      await Api.activateStylePack(id);
      if (!this.viewActive) return;
      notify('已切换为当前风格');
      await this.reload();
    } catch (error) {
      if (this.viewActive) { this.error = (error as Error).message; notify('切换失败：' + this.error); }
    } finally { this.busy = false; }
  }

  private async exportZip(id: string, name: string): Promise<void> {
    if (id.length === 0 || this.busy) return;
    this.busy = true;
    try { const note: string = await Api.exportStylePack(id, name); if (this.viewActive) notify(note); }
    catch (error) { if (this.viewActive) { this.error = (error as Error).message; notify('导出失败：' + this.error); } }
    finally { this.busy = false; }
  }

  private async importZip(): Promise<void> {
    if (this.busy) return;
    this.busy = true;
    try {
      const note: string = await Api.importStylePack();
      if (!this.viewActive) return;
      notify(note);
      await this.reload();
    } catch (error) {
      if (this.viewActive) { this.error = (error as Error).message; notify('导入失败：' + this.error); }
    } finally { this.busy = false; }
  }

  private async resetBuiltinSelected(): Promise<void> {
    const id: string = this.selectedId;
    if (id.length === 0 || this.busy || !this.detailBuiltIn()) return;
    this.busy = true;
    try {
      await Api.resetBuiltinStylePack(id);
      if (!this.viewActive) return;
      notify('已恢复内置风格默认提示词');
      await this.reload();
    } catch (error) {
      if (this.viewActive) { this.error = (error as Error).message; notify('恢复失败：' + this.error); }
    } finally { this.busy = false; }
  }

  private async removeSelected(): Promise<void> {
    const id: string = this.selectedId;
    if (id.length === 0 || this.busy || this.detailBuiltIn()) return;
    this.busy = true;
    try {
      await Api.deleteStylePack(id);
      if (!this.viewActive) return;
      if (this.selectedId === id) { this.selectedId = ''; this.sheetOpen = false; }
      notify('已删除风格包');
      await this.reload();
    } catch (error) {
      if (this.viewActive) { this.error = (error as Error).message; notify('删除失败：' + this.error); }
    } finally { this.busy = false; }
  }

  // ── 风格市场 ──────────────────────────────────────────────────────────────

  private async reloadMarket(): Promise<void> {
    if (this.marketSearchTimer >= 0) clearTimeout(this.marketSearchTimer);
    this.marketSearchTimer = -1;
    const generation: number = ++this.marketGeneration;
    const query: string = this.marketQuery;
    const sort: string = this.marketSort;
    this.marketLoading = true;
    try {
      const auth = await Api.marketplaceAuthStatus();
      if (!this.viewActive || generation !== this.marketGeneration) return;
      if (!this.loginBusy) {
        this.signedIn = auth.signedIn === true;
        this.loginState = this.signedIn
          ? (this.loginState.startsWith('已登录') ? this.loginState : '已登录 GitHub') : '';
      }
      const packs: MarketplacePack[] = await Api.marketplace(query, sort);
      if (!this.viewActive || generation !== this.marketGeneration) return;
      this.marketPacks = packs;
      this.marketLoaded = true;
      this.error = '';
    } catch (error) {
      if (this.viewActive && generation === this.marketGeneration) this.error = (error as Error).message;
    } finally {
      if (generation === this.marketGeneration) this.marketLoading = false;
    }
  }

  private marketSortIndex(): number {
    return this.marketSort === 'new' ? 1 : 0;
  }

  private loginLabel(): string {
    if (this.loginState.length > 0) return this.loginState;
    return 'GitHub 登录';
  }

  private waitLogin(ms: number): Promise<void> {
    return new Promise<void>((resolve) => {
      this.loginWake = resolve;
      this.loginTimer = setTimeout(() => {
        this.loginTimer = -1;
        this.loginWake = undefined;
        resolve();
      }, ms);
    });
  }

  private cancelLogin(): void {
    this.loginGeneration++;
    if (this.loginTimer >= 0) clearTimeout(this.loginTimer);
    this.loginTimer = -1;
    const wake = this.loginWake;
    this.loginWake = undefined;
    if (wake !== undefined) wake();
    const flowId: string = this.loginFlowId;
    this.loginFlowId = '';
    this.loginBusy = false;
    this.loginCode = '';
    if (!this.signedIn) this.loginState = '';
    if (flowId.length > 0) Api.githubDeviceFlowCancel(flowId).catch(() => {});
  }

  private async startLogin(): Promise<void> {
    if (this.loginBusy || this.busy || !this.viewActive) return;
    const generation: number = ++this.loginGeneration;
    this.loginBusy = true;
    try {
      const flow = await Api.githubDeviceFlowStart();
      if (!this.viewActive || generation !== this.loginGeneration) {
        Api.githubDeviceFlowCancel(flow.flowId).catch(() => {});
        return;
      }
      this.loginFlowId = flow.flowId;
      this.loginCode = flow.userCode;
      this.loginState = '等待授权…';
      try { await Api.copyText(flow.userCode); } catch (error) { notify('请手动复制配对码'); }
      if (!this.viewActive || generation !== this.loginGeneration) return;
      await Api.openExternalUrl(flow.verificationUri);
      if (!this.viewActive || generation !== this.loginGeneration) return;
      const expiresSecs: number = flow.expiresInSecs ?? flow.expiresIn ?? 900;
      const intervalSecs: number = flow.intervalSecs ?? flow.interval ?? 5;
      const expiresAt: number = Date.now() + Math.max(1, expiresSecs) * 1000;
      let intervalMs: number = Math.max(2, intervalSecs) * 1000;
      while (Date.now() < expiresAt) {
        await this.waitLogin(Math.min(intervalMs, Math.max(1, expiresAt - Date.now())));
        if (!this.viewActive || generation !== this.loginGeneration) return;
        if (Date.now() >= expiresAt) break;
        const poll = await Api.githubDeviceFlowPoll(flow.flowId);
        if (!this.viewActive || generation !== this.loginGeneration) return;
        if (poll.kind === 'authorized') {
          this.signedIn = true;
          this.loginState = '已登录 ' + (poll.login ?? 'GitHub');
          this.loginCode = '';
          this.loginFlowId = '';
          this.loginBusy = false;
          notify('已登录 GitHub');
          await this.reloadMarket();
          return;
        }
        if (poll.kind === 'slowDown') intervalMs += 5000;
        if (poll.kind === 'error') throw new Error(poll.message ?? '授权未完成，请重试');
      }
      throw new Error('登录已过期，请重试');
    } catch (error) {
      if (this.viewActive && generation === this.loginGeneration) {
        this.loginState = '';
        this.loginCode = '';
        this.error = (error as Error).message;
        notify('登录不可用：' + this.error);
      }
    } finally {
      if (generation === this.loginGeneration) {
        const flowId: string = this.loginFlowId;
        this.loginFlowId = '';
        this.loginBusy = false;
        if (flowId.length > 0) Api.githubDeviceFlowCancel(flowId).catch(() => {});
      }
    }
  }

  private async toggleLogin(): Promise<void> {
    if (this.loginBusy || this.busy) return;
    if (this.signedIn) {
      this.loginBusy = true;
      try {
        await Api.marketplaceLogout();
        if (!this.viewActive) return;
        this.signedIn = false;
        this.loginState = '';
        this.loginCode = '';
        this.likedIds = [];
        notify('已退出 GitHub');
      } catch (error) {
        if (this.viewActive) { this.error = (error as Error).message; notify('退出失败：' + this.error); }
      } finally { this.loginBusy = false; }
      if (this.viewActive) await this.reloadMarket();
      return;
    }
    await this.startLogin();
  }

  private marketDetailIndex(): number {
    for (let index = 0; index < this.marketPacks.length; index++) {
      if (this.marketPacks[index].id === this.marketSelectedId) return index;
    }
    return -1;
  }

  private marketDetailName(): string {
    const index: number = this.marketDetailIndex();
    return index < 0 ? '' : this.marketPacks[index].name;
  }

  private marketDetailDescription(): string {
    const index: number = this.marketDetailIndex();
    if (index < 0) return '';
    return this.marketPacks[index].description ?? '';
  }

  private marketDetailInstalled(): boolean {
    const index: number = this.marketDetailIndex();
    return index >= 0 && this.marketInstalled(this.marketPacks[index]);
  }

  private marketDetailMeta(): string {
    const index: number = this.marketDetailIndex();
    if (index < 0) return '';
    const pack: MarketplacePack = this.marketPacks[index];
    return (pack.authorLogin || '社区') + ' · ' + (pack.downloadCount ?? 0).toString() + ' 次下载 · ' +
      (pack.likeCount ?? 0).toString() + ' 赞';
  }

  private selectMarketPack(id: string): void {
    if (this.busy) return;
    this.marketSelectedId = id;
    if (!this.isWide()) this.openSheet(SHEET_MARKET);
  }

  private marketInstalled(pack: MarketplacePack): boolean {
    return StyleEditing.installed(pack, this.packs);
  }

  private marketLiked(id: string): boolean {
    return this.likedIds.indexOf(id) >= 0;
  }

  private async install(id: string): Promise<void> {
    if (id.length === 0 || this.busy) return;
    const pack: MarketplacePack | undefined = this.marketPacks.find((item: MarketplacePack) => item.id === id);
    if (pack === undefined || this.marketInstalled(pack)) return;
    this.busy = true;
    try {
      await Api.installMarketplacePack(id);
      if (!this.viewActive) return;
      notify('已安装到本机');
      await this.reload();
      await this.reloadMarket();
    } catch (error) {
      if (this.viewActive) { this.error = (error as Error).message; notify('安装失败：' + this.error); }
    } finally { this.busy = false; }
  }

  private async like(id: string): Promise<void> {
    if (id.length === 0 || this.busy) return;
    this.busy = true;
    try {
      const result = await Api.likePack(id);
      if (!this.viewActive) return;
      this.likedIds = this.likedIds.filter((old: string) => old !== id);
      if (result.alreadyLiked) this.likedIds = this.likedIds.concat([id]);
      this.marketPacks = this.marketPacks.map((pack: MarketplacePack) => {
        if (pack.id === id) pack.likeCount = result.likeCount;
        return pack;
      });
      notify(result.alreadyLiked ? '已点赞' : '已取消点赞');
    } catch (error) {
      if (this.viewActive) { this.error = (error as Error).message; notify('点赞操作失败：' + this.error); }
    } finally { this.busy = false; }
  }

  build() {
    PageScaffold({
      title: this.segment === 0 ? '我的风格' : '风格市场',
      actionLabel: this.actionLabel(),
      actionSymbol: this.actionSymbol(),
      scrollable: false,
      gap: Space.sm,
      onAction: () => this.triggerAction()
    }) {
      if (this.error.length > 0) {
        ErrorNote({
          message: '风格库暂不可用，请稍后重试。',
          onRetry: () => {
            if (this.segment === 1) {
              this.reloadMarket();
            } else {
              this.reload();
            }
          }
        })
      }
      this.filterRow()
      if (this.isWide()) {
        this.wideBody()
      } else {
        this.narrowBody()
      }
      this.sheetHost()
    }
  }

  /**
   * 一条搜索行 + 段切换 + 段内独占的筛选/动作。搜索框始终单独一行，
   * 动作按钮一律走 `Flex` 换行，不会把输入框挤窄。
   */
  @Builder
  filterRow() {
    Column({ space: Space.xs }) {
      SearchField({
        placeholder: this.searchPlaceholder(),
        value: this.searchValue(),
        onInput: (value: string) => {
          this.onSearchInput(value);
        }
      })

      if (this.segment === 1) {
        if (this.loginCode.length > 0) {
          Banner({
            title: '在浏览器输入配对码',
            text: this.loginCode,
            action: '复制配对码',
            actionSymbol: 'copy',
            onAction: () => {
              Api.copyText(this.loginCode);
              notify('已复制配对码');
            }
          })
        }

      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
  }

  /** Market footnote rows: account state and ordering live under the list. */
  @Builder
  marketFooter() {
    Row() {
      SymbolGlyph(symbolOf('person'))
        .fontSize(Size.iconSmall)
        .fontColor([Palette.text2()])
      TextButton({
        label: this.loginBusy ? '取消授权' : this.loginLabel(),
        tone: 'quiet',
        usable: !this.busy,
        onTap: () => {
          if (this.loginBusy) this.cancelLogin();
          else this.toggleLogin();
        }
      })
      Blank()
      TextButton({
        label: this.marketSort === 'new' ? '按最新' : '按热度',
        tone: 'quiet',
        onTap: () => {
          this.marketSort = this.marketSort === 'new' ? 'popular' : 'new';
          this.reloadMarket();
        }
      })
    }
    .width('100%')
    .padding({ top: Space.sm })
    .alignItems(VerticalAlign.Center)
  }

  /** iOS footnote rows under the installed list (StyleLibraryView.swift:231-235). */
  @Builder
  libraryFooter() {
    Column({ space: Space.xs }) {
      Row() {
        SymbolGlyph(symbolOf('sparkles'))
          .fontSize(Size.iconSmall)
          .fontColor([Palette.text2()])
        TextButton({
          label: '设置文字模型',
          tone: 'quiet',
          onTap: () => Nav.go(Nav.CREDENTIALS)
        })
      }
      .width('100%')
      .alignItems(VerticalAlign.Center)

      Row() {
        TextButton({
          label: '新建风格',
          tone: 'quiet',
          onTap: () => {
            this.startCreate();
          }
        })
        .id('style-create-open')
      }
      .width('100%')
    }
    .width('100%')
    .padding({ top: Space.sm })
    .alignItems(HorizontalAlign.Start)
  }

  @Builder
  narrowBody() {
    Scroll() {
      Column({ space: Space.sm }) {
        this.listBody()
        if (this.segment === 0) {
          this.libraryFooter()
        } else {
          this.marketFooter()
        }
      }
      .width('100%')
      .padding({ bottom: Layout.dockClearance() })
    }
    .width('100%')
    .layoutWeight(1)
    .align(Alignment.Top)
    .scrollBar(BarState.Off)
    .edgeEffect(EdgeEffect.Spring)
  }

  @Builder
  wideBody() {
    Row({ space: Space.md }) {
      Column() {
        Scroll() {
          Column({ space: Space.sm }) {
            this.listBody()
            if (this.segment === 0) {
              this.libraryFooter()
            } else {
              this.marketFooter()
            }
          }
          .width('100%')
          .padding({ bottom: Layout.dockClearance() })
        }
        .width('100%')
        .layoutWeight(1)
        .align(Alignment.Top)
        .scrollBar(BarState.Off)
        .edgeEffect(EdgeEffect.Spring)
      }
      .layoutWeight(5)
      .height('100%')

      this.detailPane()
    }
    .width('100%')
    .layoutWeight(1)
    .alignItems(VerticalAlign.Top)
  }

  /** 右栏：按段取各自的选中项；没有选中就提示。 */
  @Builder
  detailPane() {
    Column() {
      if (this.segment === 1) {
        if (this.marketDetailIndex() >= 0) {
          Scroll() {
            Column() {
              Surface({ inset: Space.md }) {
                this.marketDetailBody()
              }
            }
            .width('100%')
            .padding({ bottom: Layout.dockClearance() })
          }
          .width('100%')
          .layoutWeight(1)
          .align(Alignment.Top)
          .scrollBar(BarState.Off)
          .edgeEffect(EdgeEffect.Spring)
        } else {
          Surface({ inset: Space.md }) {
            EmptyHint({
              symbol: 'marketplace',
              title: '选一个社区风格包',
              hint: '右侧显示作者、下载量与安装、点赞操作'
            })
          }
        }
      } else {
        if (this.detailIndex() >= 0) {
          Scroll() {
            Column() {
              Surface({ inset: Space.md }) {
                this.localDetailBody()
              }
            }
            .width('100%')
            .padding({ bottom: Layout.dockClearance() })
          }
          .width('100%')
          .layoutWeight(1)
          .align(Alignment.Top)
          .scrollBar(BarState.Off)
          .edgeEffect(EdgeEffect.Spring)
        } else {
            EmptyHint({
              symbol: 'style',
              title: '选一个风格包',
              hint: '右侧可以改提示词、设为当前风格或导出 ZIP'
            })
        }
      }
    }
    .layoutWeight(4)
    .height('100%')
  }

  @Builder
  listBody() {
    if (this.segment === 1) {
      this.marketListBody()
    } else {
      this.localListBody()
    }
  }

  @Builder
  localListBody() {
    if (this.loading && this.packs.length === 0) {
      Skeleton({ rows: 4 })
    } else if (this.filtered().length === 0) {
      EmptyHint({
        symbol: this.packs.length === 0 ? 'style' : 'search',
        title: this.packs.length === 0 ? '还没有添加风格' : '没有匹配的风格',
        hint: this.packs.length === 0
          ? '切到「风格市场」安装一个，或用右上角加号导入 ZIP。'
          : '试试其他关键词。'
      })
    } else {
      Column({ space: Space.sm }) {
        ForEach(this.filtered(), (pack: StylePack) => {
          StyleSummaryCard({
            name: pack.name,
            summary: pack.description ?? '',
            metadata: (pack.kind === 'builtin' ? '内置风格' : '自定义风格') +
              (pack.enabled === false ? ' · 已停用' : ''),
            badge: pack.kind === 'builtin' && pack.id !== this.active ? '内置' : '',
            badgeTone: 'neutral',
            selected: pack.id === this.active,
            primaryLabel: pack.id === this.active ? '使用中' : '使用',
            primaryEnabled: pack.enabled !== false && pack.id !== this.active && !this.busy,
            secondaryLabel: '',
            muted: pack.enabled === false,
            onOpen: () => this.selectPack(pack.id),
            onPrimary: () => {
              this.activate(pack.id);
            },
            onSecondary: () => {
              this.exportZip(pack.id, pack.name);
            }
          })
        }, (pack: StylePack) => pack.id)
      }
      .width('100%')
    }
  }

  @Builder
  marketListBody() {
    if (this.marketLoading && this.marketPacks.length === 0) {
      Skeleton({ rows: 4 })
    } else if (this.marketPacks.length === 0) {
      EmptyHint({
        symbol: 'marketplace',
        title: this.marketLoading ? '正在加载…' : '没有匹配的风格',
        hint: '市场需要网络与 GitHub 账号；也可以换个关键词再试。'
      })
    } else {
      Column({ space: Space.sm }) {
        ForEach(this.marketPacks, (pack: MarketplacePack) => {
          StyleSummaryCard({
            name: pack.name,
            summary: pack.description ?? '',
            // iOS 市场行把作者放进元信息，详情页再补「下载 · 喜欢」。
            metadata: (pack.authorLogin || '社区') + ' · 下载 ' + (pack.downloadCount ?? 0).toString() +
              ' · 喜欢 ' + (pack.likeCount ?? 0).toString(),
            badge: this.marketInstalled(pack) ? '已安装' : '',
            badgeTone: 'success',
            primaryLabel: this.marketInstalled(pack) ? '' : '安装',
            primaryEnabled: !this.busy,
            secondaryLabel: this.marketLiked(pack.id) ? '取消点赞' : '点赞',
            selected: pack.id === this.marketSelectedId,
            onOpen: () => this.selectMarketPack(pack.id),
            onPrimary: () => {
              this.install(pack.id);
            },
            onSecondary: () => {
              this.like(pack.id);
            }
          })
        }, (pack: MarketplacePack) => pack.id)
      }
      .width('100%')
    }
  }

  /** 本地风格包详情，宽屏右栏与窄屏 sheet 共用。 */
  @Builder
  localDetailBody() {
    Column({ space: Space.sm }) {
      Text(this.detailName())
        .fontSize(Type.cardTitle)
        .lineHeight(Leading.cardTitle)
        .fontWeight(FontWeight.Medium)
        .fontColor(Palette.text1())
        .width('100%')

      Row({ space: Space.xs }) {
        if (this.detailActive()) {
          Tag({ text: '使用中', tone: 'accent' })
        }
        if (this.detailBuiltIn()) {
          Tag({ text: '内置', tone: 'neutral' })
        } else {
          Tag({ text: '自定义', tone: 'neutral' })
        }
        Tag({
          text: this.detailEnabled() ? '已启用' : '已停用',
          tone: this.detailEnabled() ? 'success' : 'neutral'
        })
      }
      .width('100%')
      .justifyContent(FlexAlign.Start)

      if (this.detailDescription().length > 0) {
        Text(this.detailDescription())
          .fontSize(Type.label)
          .lineHeight(Leading.label)
          .fontColor(Palette.text2())
          .width('100%')
      }

      Column({ space: Space.xxs }) {
        Text('系统提示词')
          .fontSize(Type.caption)
          .lineHeight(Leading.caption)
          .fontWeight(FontWeight.Medium)
          .fontColor(Palette.text3())
          .width('100%')
        Text(this.detailPrompt().length > 0 ? this.detailPrompt() : '（还没有提示词）')
          .fontSize(Type.label)
          .lineHeight(Leading.label)
          .fontColor(Palette.text2())
          .maxLines(8)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .width('100%')
      }
      .width('100%')
      .alignItems(HorizontalAlign.Start)
      .padding(Space.sm)
      .backgroundColor(Palette.cardAlt())
      .borderRadius(Radius.control)

      SettingSwitch({
        label: '启用这个风格',
        hint: '停用后不会出现在切换列表里',
        checked: this.detailEnabled(),
        showDivider: false,
        onToggle: (value: boolean) => {
          this.setEnabled(value);
        }
      })

      Row({ space: Space.sm }) {
        if (!this.detailActive()) {
          TonalButton({
            label: '设为当前风格',
            symbol: 'check',
            usable: !this.busy && this.detailEnabled(),
            onTap: () => {
              this.activate(this.selectedId);
            }
          })
        }
        TonalButton({
          label: '编辑提示词',
          symbol: 'edit',
          usable: !this.busy,
          onTap: () => this.startEdit()
        })
        .id('style-edit-open')
      }
      .width('100%')
      .justifyContent(FlexAlign.Start)

      Row({ space: Space.sm }) {
        TextButton({
          label: '导出 ZIP',
          symbol: 'copy',
          tone: 'quiet',
          onTap: () => {
            this.exportZip(this.selectedId, this.detailName());
          }
        })
        if (this.detailBuiltIn()) {
          TextButton({
            label: '恢复默认',
            symbol: 'refresh',
            tone: 'quiet',
            usable: !this.busy,
            onTap: () => {
              this.resetBuiltinSelected();
            }
          })
        } else {
          TextButton({
            label: '删除',
            symbol: 'trash',
            tone: 'danger',
            usable: !this.busy,
            onTap: () => {
              this.openSheet(SHEET_CONFIRM);
            }
          })
        }
        if (!this.isWide()) {
          TextButton({
            label: '关闭',
            tone: 'quiet',
            onTap: () => {
              this.closeSheet();
            }
          })
        }
      }
      .width('100%')
      .justifyContent(FlexAlign.Start)
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
  }

  /** 市场风格包详情，宽屏右栏与窄屏 sheet 共用。 */
  @Builder
  marketDetailBody() {
    Column({ space: Space.sm }) {
      Text(this.marketDetailName())
        .fontSize(Type.cardTitle)
        .lineHeight(Leading.cardTitle)
        .fontWeight(FontWeight.Medium)
        .fontColor(Palette.text1())
        .width('100%')

      Row({ space: Space.xs }) {
        Tag({
          text: this.marketDetailInstalled() ? '已安装' : '未安装',
          tone: this.marketDetailInstalled() ? 'success' : 'neutral'
        })
      }
      .width('100%')
      .justifyContent(FlexAlign.Start)

      if (this.marketDetailDescription().length > 0) {
        Text(this.marketDetailDescription())
          .fontSize(Type.label)
          .lineHeight(Leading.label)
          .fontColor(Palette.text2())
          .width('100%')
      }

      Text(this.marketDetailMeta())
        .fontSize(Type.caption)
        .lineHeight(Leading.caption)
        .fontColor(Palette.text3())
        .width('100%')

      Row({ space: Space.sm }) {
        if (!this.marketDetailInstalled()) {
          PrimaryButton({
            label: this.busy ? '安装中…' : '安装到本机',
            usable: !this.busy,
            onTap: () => {
              this.install(this.marketSelectedId);
            }
          })
        }
        TonalButton({
          label: this.marketLiked(this.marketSelectedId) ? '取消点赞' : '点赞',
          symbol: 'sparkles',
          usable: !this.busy,
          onTap: () => {
            this.like(this.marketSelectedId);
          }
        })
        if (!this.isWide()) {
          TextButton({
            label: '关闭',
            tone: 'quiet',
            onTap: () => {
              this.closeSheet();
            }
          })
        }
      }
      .width('100%')
      .justifyContent(FlexAlign.Start)
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
  }

  @Builder
  createBody() {
    Column({ space: Space.md }) {
      SheetTitle({
        title: '新建风格',
        hint: '提示词决定润色的角色与语气'
      })
      InputField({
        label: '名称',
        placeholder: '例如「工作邮件」',
        value: this.newName,
        onInput: (value: string) => {
          this.newName = value;
        }
      })
      InputField({
        label: '系统提示词',
        placeholder: '你是一位…，把口语整理成…',
        value: this.newPrompt,
        multiline: true,
        hint: '留空也可以，之后再补',
        onInput: (value: string) => {
          this.newPrompt = value;
        }
      })
      Row({ space: Space.sm }) {
        PrimaryButton({
          label: this.busy ? '保存中…' : '保存风格',
          usable: !this.busy && this.newName.trim().length > 0,
          onTap: () => {
            this.createPack();
          }
        })
        .id('style-create-save')
        TextButton({
          label: '取消',
          tone: 'quiet',
          usable: !this.busy,
          onTap: () => {
            this.closeSheet();
          }
        })
      }
      .width('100%')
      .justifyContent(FlexAlign.End)
    }
    .width('100%')
  }

  @Builder
  editBody() {
    Column({ space: Space.md }) {
      SheetTitle({
        title: '编辑提示词',
        hint: this.detailName()
      })
      InputField({
        label: '系统提示词',
        placeholder: '你是一位…，把口语整理成…',
        value: this.draftPrompt,
        multiline: true,
        onInput: (value: string) => {
          this.draftPrompt = value;
        }
      })
      Row({ space: Space.sm }) {
        PrimaryButton({
          label: this.busy ? '保存中…' : '保存',
          usable: !this.busy,
          onTap: () => {
            this.savePrompt();
          }
        })
        .id('style-edit-save')
        TextButton({
          label: '取消',
          tone: 'quiet',
          usable: !this.busy,
          onTap: () => {
            this.openSheet(SHEET_DETAIL);
          }
        })
      }
      .width('100%')
      .justifyContent(FlexAlign.End)
    }
    .width('100%')
  }

  @Builder
  confirmBody() {
    Column({ space: Space.md }) {
      SheetTitle({
        title: '删除风格包',
        hint: '删除后提示词与设置都无法恢复；内置风格可以重新安装'
      })
      Text('「' + this.detailName() + '」将从本机移除。')
        .fontSize(Type.body)
        .lineHeight(Leading.body)
        .fontColor(Palette.text2())
        .width('100%')
      Row({ space: Space.sm }) {
        PrimaryButton({
          label: '删除',
          destructive: true,
          usable: !this.busy,
          onTap: () => {
            this.removeSelected();
          }
        })
        TextButton({
          label: '取消',
          tone: 'quiet',
          usable: !this.busy,
          onTap: () => {
            this.openSheet(SHEET_DETAIL);
          }
        })
      }
      .width('100%')
      .justifyContent(FlexAlign.End)
    }
    .width('100%')
  }

  @Builder
  sheetBody() {
    Column({ space: Space.md }) {
      if (this.sheetMode === SHEET_CREATE) {
        this.createBody()
      } else if (this.sheetMode === SHEET_DETAIL) {
        SheetTitle({
          title: '风格详情',
          actionLabel: '完成',
          onAction: () => {
            this.closeSheet();
          }
        })
        .id('style-detail-done')
        this.localDetailBody()
      } else if (this.sheetMode === SHEET_EDIT) {
        this.editBody()
      } else if (this.sheetMode === SHEET_CONFIRM) {
        this.confirmBody()
      } else if (this.sheetMode === SHEET_MARKET) {
        this.marketDetailBody()
      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
    .padding({ left: Space.md, right: Space.md, top: Space.sm, bottom: Space.xl })
  }

  @Builder
  sheetHost() {
    Row()
      .width(0)
      .height(0)
      .bindSheet($$this.sheetOpen, this.sheetBody, {
        preferType: this.isWide() ? SheetType.CENTER : SheetType.BOTTOM,
        width: this.isWide() ? 560 : '100%',
        height: this.isWide() ? SheetSize.FIT_CONTENT : SheetSize.MEDIUM,
        dragBar: true,
        showClose: this.sheetMode !== SHEET_DETAIL,
        radius: LengthMetrics.vp(Radius.sheet),
        onDisappear: () => {
          this.sheetOpen = false;
        }
      })
  }
}
