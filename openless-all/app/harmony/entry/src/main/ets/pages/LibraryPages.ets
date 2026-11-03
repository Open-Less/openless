import { Fmt } from '../common/Format';
import { Layout, Leading, Palette, Radius, Space, Type } from '../common/Theme';
import {
  Banner,
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
import { Api, PendingCorrection, StylePackPayload, VocabPreset } from '../data/Api';
import { CorrectionRule, DictionaryEntry, MarketplacePack, SettingsSnapshot, StylePack } from '../data/Types';

/**
 * Library surfaces of the v2 UI: 词典 / 纠正规则 / 风格 / 风格市场.
 *
 * All four are list-shaped, so they share one skeleton: a `PageScaffold` title,
 * a search field on its own line, a `Segmented` filter, a `Flex`-wrapped action
 * row (never a single overflowing `Row`), then a `Group` of `RowItem`s. Rows only
 * navigate: every mutation (enable, delete, edit, install, like) happens in a
 * detail body that wide windows pin to the right column and compact windows lift
 * into a `bindSheet`. One sheet anchor per page switches content through
 * `sheetMode`, so two sheets can never stack.
 */

const SHEET_ADD: string = 'add';
const SHEET_DETAIL: string = 'detail';
const SHEET_PRESETS: string = 'presets';
const SHEET_CREATE: string = 'create';
const SHEET_EDIT: string = 'edit';
const SHEET_CONFIRM: string = 'confirm';
const SHEET_SUGGEST: string = 'suggest';

const LIBRARY_FILTERS: string[] = ['全部', '已启用', '已停用'];
const MARKET_SORTS: string[] = ['按热度', '最新'];

/**
 * 词典：本机词条的增删启停 + 只读预设词组。
 *
 * `xl`/`lg` 时左侧列表右侧详情；窄屏点行把同一份详情抬进半模态。
 */
@Component
export struct VocabPage {
  @StorageLink('bp') bp: string = 'md';

  @State private entries: DictionaryEntry[] = [];
  @State private presets: VocabPreset[] = [];
  @State private query: string = '';
  @State private filter: number = 0;
  @State private error: string = '';
  @State private loading: boolean = false;
  @State private busy: boolean = false;
  @State private selectedId: string = '';
  @State private sheetOpen: boolean = false;
  @State private sheetMode: string = SHEET_ADD;
  @State private draft: string = '';

  aboutToAppear(): void {
    this.reload();
  }

  private isWide(): boolean {
    return this.bp === Layout.LG || this.bp === Layout.XL;
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

  private filtered(): DictionaryEntry[] {
    const needle: string = this.query.trim().toLowerCase();
    const result: DictionaryEntry[] = [];
    for (let index = 0; index < this.entries.length; index++) {
      const entry: DictionaryEntry = this.entries[index];
      if (this.filter === 1 && entry.enabled !== true) continue;
      if (this.filter === 2 && entry.enabled === true) continue;
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

  private openSheet(mode: string): void {
    this.sheetMode = mode;
    this.sheetOpen = true;
  }

  private closeSheet(): void {
    this.sheetOpen = false;
  }

  private selectEntry(id: string): void {
    this.selectedId = id;
    if (!this.isWide()) this.openSheet(SHEET_DETAIL);
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
    PageScaffold({
      title: '词典',
      subtitle: this.entries.length.toString() + ' 个词条 · 转写时优先命中',
      actionLabel: '纠正规则',
      actionSymbol: 'corrections',
      scrollable: false,
      gap: Space.sm,
      onAction: () => Nav.go(Nav.CORRECTIONS)
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

  /**
   * Search stays on its own line, filter and actions each own a line, so nothing
   * can squeeze the input down to a few visible characters.
   */
  @Builder
  filterRow() {
    Column({ space: Space.xs }) {
      SearchField({
        placeholder: '搜索词条',
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
      Flex({ wrap: FlexWrap.Wrap, justifyContent: FlexAlign.Start }) {
        Row() {
          TonalButton({
            label: '添加词条',
            symbol: 'add',
            onTap: () => {
              this.draft = '';
              this.openSheet(SHEET_ADD);
            }
          })
        }
        .padding({ right: Space.xs, bottom: Space.xxs })

        Row() {
          TonalButton({
            label: '预设词组',
            symbol: 'list',
            onTap: () => {
              this.openSheet(SHEET_PRESETS);
            }
          })
        }
        .padding({ right: Space.xs, bottom: Space.xxs })
      }
      .width('100%')
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
      .padding({ bottom: Space.lg })
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
          .padding({ bottom: Space.lg })
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
              symbol: 'vocab',
              title: '选一个词条',
              hint: '右侧会显示命中次数、备注与启停状态'
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
    if (this.loading && this.entries.length === 0) {
      Skeleton({ rows: 4 })
    } else if (this.filtered().length === 0) {
      if (this.entries.length === 0) {
        EmptyHint({
          symbol: 'vocab',
          title: '词典还是空的',
          hint: '把常被听错的名字加进来，转写会优先匹配'
        })
      } else {
        EmptyHint({
          symbol: 'search',
          title: '没有匹配的词条',
          hint: '换个关键词，或把筛选切回「全部」'
        })
      }
    } else {
      Group() {
        ForEach(this.filtered(), (entry: DictionaryEntry, index: number) => {
          RowItem({
            symbol: 'vocab',
            title: entry.phrase,
            subtitle: (entry.hits ?? 0).toString() + ' 次命中' +
              (entry.note !== undefined && entry.note !== null && entry.note.length > 0 ? ' · ' + entry.note : ''),
            value: entry.enabled === true ? '已启用' : '已停用',
            muted: entry.enabled !== true,
            showChevron: true,
            showDivider: index < this.filtered().length - 1,
            onTap: () => {
              this.selectEntry(entry.id);
            }
          })
        }, (entry: DictionaryEntry) => entry.id)
      }
    }
  }

  /** Shared by the wide right column and the compact sheet. */
  @Builder
  detailBody() {
    Column({ space: Space.sm }) {
      Text(this.detailPhrase())
        .fontSize(Type.cardTitle)
        .lineHeight(Leading.cardTitle)
        .fontWeight(FontWeight.Medium)
        .fontColor(Palette.text1())
        .width('100%')

      Row({ space: Space.xs }) {
        Tag({
          text: this.detailEnabled() ? '已启用' : '已停用',
          tone: this.detailEnabled() ? 'success' : 'neutral'
        })
        Tag({ text: this.detailHits(), tone: 'accent' })
      }
      .width('100%')
      .justifyContent(FlexAlign.Start)

      if (this.detailNote().length > 0) {
        Text(this.detailNote())
          .fontSize(Type.label)
          .lineHeight(Leading.label)
          .fontColor(Palette.text2())
          .width('100%')
      }

      Text('加入于 ' + this.detailSince())
        .fontSize(Type.caption)
        .lineHeight(Leading.caption)
        .fontColor(Palette.text3())
        .width('100%')

      SettingSwitch({
        label: '启用该词条',
        hint: '停用后转写不再优先匹配，词条仍会保留',
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
          label: '删除词条',
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
        title: '添加词条',
        hint: '常被听错的人名、产品名，转写时会优先匹配'
      })
      InputField({
        label: '词条',
        placeholder: '例如「谐音输入法」',
        value: this.draft,
        hint: '一个词条一行，回车可以直接提交',
        onInput: (value: string) => {
          this.draft = value;
        }
      })
      Row({ space: Space.sm }) {
        PrimaryButton({
          label: this.busy ? '添加中…' : '添加',
          usable: !this.busy && this.draft.trim().length > 0,
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
  presetsBody() {
    Column({ space: Space.md }) {
      SheetTitle({
        title: '预设词组',
        hint: '来自加密文档库，随云同步一起更新，共 ' + this.presets.length.toString() + ' 组'
      })
      if (this.presets.length === 0) {
        EmptyHint({
          symbol: 'list',
          title: '还没有预设词组',
          hint: '文档库里的词组整理好之后会自动出现在这里'
        })
      } else {
        Group() {
          ForEach(this.presets, (preset: VocabPreset, index: number) => {
            RowItem({
              title: preset.name !== undefined && preset.name !== null && preset.name.length > 0
                ? preset.name : preset.id,
              subtitle: (preset.phrases === undefined ? 0 : preset.phrases.length).toString() + ' 条 · ' +
                (preset.origin ?? '本机'),
              value: preset.enabled === false ? '已停用' : '启用中',
              muted: preset.enabled === false,
              showDivider: index < this.presets.length - 1
            })
          }, (preset: VocabPreset) => preset.id)
        }
      }
    }
    .width('100%')
  }

  @Builder
  sheetBody() {
    Column({ space: Space.md }) {
      if (this.sheetMode === SHEET_ADD) {
        this.addBody()
      } else if (this.sheetMode === SHEET_PRESETS) {
        this.presetsBody()
      } else if (this.sheetMode === SHEET_DETAIL) {
        this.detailBody()
      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
    .padding({ left: Space.md, right: Space.md, top: Space.sm, bottom: Space.xl })
  }

  /** Single anchor: `sheetMode` swaps the body, so two sheets never stack. */
  @Builder
  sheetHost() {
    Row()
      .width(0)
      .height(0)
      .bindSheet($$this.sheetOpen, this.sheetBody, {
        height: SheetSize.MEDIUM,
        dragBar: true,
        showClose: true,
        onDisappear: () => {
          this.sheetOpen = false;
        }
      })
  }
}

/**
 * 纠正规则：规则增删启停 + 模型学习建议的接受/忽略。
 */
@Component
export struct CorrectionsPage {
  @StorageLink('bp') bp: string = 'md';

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
    return this.bp === Layout.LG || this.bp === Layout.XL;
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
    Column({ space: Space.xs }) {
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
      Flex({ wrap: FlexWrap.Wrap, justifyContent: FlexAlign.Start }) {
        Row() {
          TonalButton({
            label: this.pending.length > 0 ? '学习建议 ' + this.pending.length.toString() : '学习建议',
            symbol: 'sparkles',
            usable: this.pending.length > 0,
            onTap: () => {
              this.openSheet(SHEET_SUGGEST);
            }
          })
        }
        .padding({ right: Space.xs, bottom: Space.xxs })

        Row() {
          TonalButton({
            label: '新建规则',
            symbol: 'add',
            onTap: () => {
              this.pattern = '';
              this.replacement = '';
              this.openSheet(SHEET_ADD);
            }
          })
        }
        .padding({ right: Space.xs, bottom: Space.xxs })
      }
      .width('100%')
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
      .padding({ bottom: Space.lg })
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
          .padding({ bottom: Space.lg })
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
      Group() {
        ForEach(this.filtered(), (rule: CorrectionRule, index: number) => {
          RowItem({
            symbol: 'corrections',
            title: rule.pattern + ' → ' + rule.replacement,
            subtitle: (rule.source === 'learned' ? '自动学习 · ' : '手动添加 · ') +
              Fmt.relative(rule.createdAt ?? ''),
            value: rule.enabled === true ? '已启用' : '已停用',
            muted: rule.enabled !== true,
            showChevron: true,
            showDivider: index < this.filtered().length - 1,
            onTap: () => {
              this.selectRule(rule.id);
            }
          })
        }, (rule: CorrectionRule) => rule.id)
      }
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
        height: SheetSize.MEDIUM,
        dragBar: true,
        showClose: true,
        onDisappear: () => {
          this.sheetOpen = false;
        }
      })
  }
}

/**
 * 风格：风格包的增删改、启停、切换当前、ZIP 导入导出。
 */
@Component
export struct StylePage {
  @StorageLink('bp') bp: string = 'md';
  @StorageLink('backendRevision') revision: number = 0;

  @State private packs: StylePack[] = [];
  @State private active: string = '';
  @State private query: string = '';
  @State private filter: number = 0;
  @State private error: string = '';
  @State private loading: boolean = false;
  @State private busy: boolean = false;
  @State private selectedId: string = '';
  @State private sheetOpen: boolean = false;
  @State private sheetMode: string = SHEET_CREATE;
  @State private newName: string = '';
  @State private newPrompt: string = '';
  @State private draftPrompt: string = '';

  aboutToAppear(): void {
    this.reload();
  }

  private isWide(): boolean {
    return this.bp === Layout.LG || this.bp === Layout.XL;
  }

  private async reload(): Promise<void> {
    this.loading = true;
    try {
      this.packs = await Api.stylePacks();
      const snapshot: SettingsSnapshot = await Api.settings();
      this.active = snapshot.preferences.activeStylePackId ?? '';
      this.error = '';
    } catch (error) {
      this.error = (error as Error).message;
    } finally {
      this.loading = false;
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
    return this.packs[index].systemPrompt ?? '';
  }

  private detailEnabled(): boolean {
    const index: number = this.detailIndex();
    return index >= 0 && this.packs[index].enabled !== false;
  }

  private detailBuiltIn(): boolean {
    const index: number = this.detailIndex();
    return index >= 0 && this.packs[index].builtIn === true;
  }

  private detailActive(): boolean {
    return this.selectedId.length > 0 && this.selectedId === this.active;
  }

  private openSheet(mode: string): void {
    this.sheetMode = mode;
    this.sheetOpen = true;
  }

  private closeSheet(): void {
    this.sheetOpen = false;
  }

  private selectPack(id: string): void {
    this.selectedId = id;
    this.draftPrompt = '';
    if (!this.isWide()) this.openSheet(SHEET_DETAIL);
  }

  private startEdit(): void {
    this.draftPrompt = this.detailPrompt();
    this.openSheet(SHEET_EDIT);
  }

  private startCreate(): void {
    this.newName = '';
    this.newPrompt = '';
    this.openSheet(SHEET_CREATE);
  }

  private async createPack(): Promise<void> {
    const name: string = this.newName.trim();
    if (name.length === 0) return;
    this.busy = true;
    try {
      await Api.saveStylePack({
        name: name,
        description: '',
        systemPrompt: this.newPrompt.trim(),
        enabled: true
      } as StylePackPayload);
      this.newName = '';
      this.newPrompt = '';
      this.closeSheet();
      notify('已创建风格「' + name + '」');
      await this.reload();
    } catch (error) {
      const message: string = (error as Error).message;
      this.error = message;
      notify('创建失败：' + message);
    } finally {
      this.busy = false;
    }
  }

  private async savePrompt(): Promise<void> {
    const index: number = this.detailIndex();
    if (index < 0) return;
    const name: string = this.packs[index].name;
    this.busy = true;
    try {
      await Api.saveStylePack({
        id: this.selectedId,
        name: name,
        description: this.packs[index].description ?? '',
        systemPrompt: this.draftPrompt,
        enabled: this.packs[index].enabled !== false
      } as StylePackPayload);
      this.closeSheet();
      notify('提示词已保存');
      await this.reload();
    } catch (error) {
      const message: string = (error as Error).message;
      this.error = message;
      notify('保存失败：' + message);
    } finally {
      this.busy = false;
    }
  }

  private async setEnabled(enabled: boolean): Promise<void> {
    const id: string = this.selectedId;
    if (id.length === 0) return;
    try {
      await Api.setStylePackEnabled(id, enabled);
      notify(enabled ? '风格已启用' : '风格已停用');
      await this.reload();
    } catch (error) {
      this.error = (error as Error).message;
      notify('操作失败：' + this.error);
    }
  }

  private async activate(): Promise<void> {
    const id: string = this.selectedId;
    if (id.length === 0) return;
    try {
      await Api.activateStylePack(id);
      notify('已切换为当前风格');
      await this.reload();
    } catch (error) {
      this.error = (error as Error).message;
      notify('切换失败：' + this.error);
    }
  }

  private async exportZip(): Promise<void> {
    const index: number = this.detailIndex();
    if (index < 0) return;
    try {
      const note: string = await Api.exportStylePack(this.selectedId, this.packs[index].name);
      notify(note);
    } catch (error) {
      this.error = (error as Error).message;
      notify('导出失败：' + this.error);
    }
  }

  private async importZip(): Promise<void> {
    try {
      const note: string = await Api.importStylePack();
      notify(note);
      await this.reload();
    } catch (error) {
      this.error = (error as Error).message;
      notify('导入失败：' + this.error);
    }
  }

  private async removeSelected(): Promise<void> {
    const id: string = this.selectedId;
    if (id.length === 0) return;
    try {
      await Api.deleteStylePack(id);
      this.selectedId = '';
      this.closeSheet();
      notify('已删除风格包');
      await this.reload();
    } catch (error) {
      this.error = (error as Error).message;
      notify('删除失败：' + this.error);
    }
  }

  build() {
    PageScaffold({
      title: '风格',
      subtitle: this.packs.length.toString() + ' 个风格包 · 决定润色的语气',
      actionLabel: '新建风格',
      actionSymbol: 'add',
      scrollable: false,
      gap: Space.sm,
      onAction: () => this.startCreate()
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
    Column({ space: Space.xs }) {
      SearchField({
        placeholder: '搜索风格包',
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
      Flex({ wrap: FlexWrap.Wrap, justifyContent: FlexAlign.Start }) {
        Row() {
          TonalButton({
            label: '风格市场',
            symbol: 'marketplace',
            onTap: () => Nav.go(Nav.STYLE)
          })
        }
        .padding({ right: Space.xs, bottom: Space.xxs })

        Row() {
          TonalButton({
            label: '从 ZIP 导入',
            symbol: 'add',
            onTap: () => {
              this.importZip();
            }
          })
        }
        .padding({ right: Space.xs, bottom: Space.xxs })

        Row() {
          TonalButton({
            label: '新建风格',
            symbol: 'add',
            onTap: () => this.startCreate()
          })
        }
        .padding({ right: Space.xs, bottom: Space.xxs })
      }
      .width('100%')
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
      .padding({ bottom: Space.lg })
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
          .padding({ bottom: Space.lg })
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
    .width('100%')
    .layoutWeight(1)
    .alignItems(VerticalAlign.Top)
  }

  @Builder
  listBody() {
    if (this.loading && this.packs.length === 0) {
      Skeleton({ rows: 4 })
    } else if (this.filtered().length === 0) {
      if (this.packs.length === 0) {
        EmptyHint({
          symbol: 'style',
          title: '还没有风格包',
          hint: '去风格市场安装一个，或创建自定义风格'
        })
      } else {
        EmptyHint({
          symbol: 'search',
          title: '没有匹配的风格包',
          hint: '换个关键词，或把筛选切回「全部」'
        })
      }
    } else {
      Group() {
        ForEach(this.filtered(), (pack: StylePack, index: number) => {
          RowItem({
            symbol: 'style',
            accentSymbol: pack.id === this.active,
            title: pack.name,
            subtitle: pack.description !== undefined && pack.description.length > 0
              ? pack.description : (pack.builtIn === true ? '内置风格包' : '自定义风格包'),
            value: pack.id === this.active ? '使用中' : (pack.builtIn === true ? '内置' : ''),
            muted: pack.enabled === false,
            showChevron: true,
            showDivider: index < this.filtered().length - 1,
            onTap: () => {
              this.selectPack(pack.id);
            }
          })
        }, (pack: StylePack) => pack.id)
      }
    }
  }

  @Builder
  detailBody() {
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
            onTap: () => {
              this.activate();
            }
          })
        }
        TonalButton({
          label: '编辑提示词',
          symbol: 'copy',
          onTap: () => this.startEdit()
        })
      }
      .width('100%')
      .justifyContent(FlexAlign.Start)

      Row({ space: Space.sm }) {
        TextButton({
          label: '导出 ZIP',
          symbol: 'copy',
          tone: 'quiet',
          onTap: () => {
            this.exportZip();
          }
        })
        TextButton({
          label: '删除',
          symbol: 'trash',
          tone: 'danger',
          onTap: () => {
            this.openSheet(SHEET_CONFIRM);
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
        TextButton({
          label: '取消',
          tone: 'quiet',
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
        this.detailBody()
      } else if (this.sheetMode === SHEET_EDIT) {
        this.editBody()
      } else if (this.sheetMode === SHEET_CONFIRM) {
        this.confirmBody()
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
        height: SheetSize.MEDIUM,
        dragBar: true,
        showClose: true,
        onDisappear: () => {
          this.sheetOpen = false;
        }
      })
  }
}

/**
 * 风格市场：列表/搜索/排序/安装/点赞 + GitHub device flow 登录态。
 */
@Component
export struct MarketplacePage {
  @StorageLink('bp') bp: string = 'md';

  @State private packs: MarketplacePack[] = [];
  @State private query: string = '';
  @State private sort: string = 'popular';
  @State private error: string = '';
  @State private loading: boolean = false;
  @State private busy: boolean = false;
  @State private loginState: string = '';
  @State private loginCode: string = '';
  @State private selectedId: string = '';
  @State private sheetOpen: boolean = false;
  @State private sheetMode: string = SHEET_DETAIL;

  aboutToAppear(): void {
    this.reload();
  }

  private isWide(): boolean {
    return this.bp === Layout.LG || this.bp === Layout.XL;
  }

  private sortIndex(): number {
    return this.sort === 'new' ? 1 : 0;
  }

  private loginLabel(): string {
    if (this.loginState.length > 0) return this.loginState;
    return 'GitHub 登录';
  }

  private async reload(): Promise<void> {
    this.loading = true;
    try {
      try {
        const auth = await Api.marketplaceAuthStatus();
        this.loginState = auth.loggedIn === true ? '已登录 ' + (auth.login ?? '') : '';
      } catch (error) {
        this.loginState = '';
      }
      this.packs = await Api.marketplace(this.query, this.sort);
      this.error = '';
    } catch (error) {
      this.error = (error as Error).message;
    } finally {
      this.loading = false;
    }
  }

  private async startLogin(): Promise<void> {
    this.busy = true;
    try {
      const flow = await Api.githubDeviceFlowStart();
      this.loginCode = flow.userCode;
      this.loginState = '等待授权…';
      Api.copyText(flow.userCode);
      let attempts: number = 0;
      while (attempts < 60) {
        attempts++;
        await new Promise<void>((resolve) => setTimeout(() => resolve(), Math.max(2, flow.interval) * 1000));
        const poll = await Api.githubDeviceFlowPoll(flow.flowId);
        if (poll.state === 'authorized') {
          this.loginState = '已登录 ' + (poll.login ?? '');
          this.loginCode = '';
          notify('已登录 GitHub');
          await this.reload();
          return;
        }
        if (poll.state === 'expired' || poll.state === 'error') {
          this.loginState = '';
          this.loginCode = '';
          notify('登录失败，请重试');
          return;
        }
      }
      this.loginState = '';
      this.loginCode = '';
      notify('登录超时');
    } catch (error) {
      this.loginState = '';
      this.loginCode = '';
      this.error = (error as Error).message;
      notify('登录不可用：' + this.error);
    } finally {
      this.busy = false;
    }
  }

  private async toggleLogin(): Promise<void> {
    if (this.loginState.length > 0) {
      try {
        await Api.marketplaceLogout();
        this.loginState = '';
        this.loginCode = '';
        notify('已退出 GitHub');
        await this.reload();
      } catch (error) {
        this.error = (error as Error).message;
        notify('退出失败：' + this.error);
      }
      return;
    }
    await this.startLogin();
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

  private detailInstalled(): boolean {
    const index: number = this.detailIndex();
    return index >= 0 && this.packs[index].installed === true;
  }

  private detailMeta(): string {
    const index: number = this.detailIndex();
    if (index < 0) return '';
    const pack: MarketplacePack = this.packs[index];
    return (pack.author ?? '社区') + ' · ' + (pack.downloads ?? 0).toString() + ' 次下载 · ' +
      (pack.likes ?? 0).toString() + ' 赞';
  }

  private selectPack(id: string): void {
    this.selectedId = id;
    if (!this.isWide()) this.openSheet();
  }

  private openSheet(): void {
    this.sheetMode = SHEET_DETAIL;
    this.sheetOpen = true;
  }

  private closeSheet(): void {
    this.sheetOpen = false;
  }

  private async install(): Promise<void> {
    const id: string = this.selectedId;
    if (id.length === 0) return;
    try {
      await Api.installMarketplacePack(id);
      notify('已安装到本机');
      await this.reload();
    } catch (error) {
      this.error = (error as Error).message;
      notify('安装失败：' + this.error);
    }
  }

  private async like(): Promise<void> {
    const id: string = this.selectedId;
    if (id.length === 0) return;
    try {
      await Api.likePack(id);
      notify('已点赞');
      await this.reload();
    } catch (error) {
      this.error = (error as Error).message;
      notify('点赞失败：' + this.error);
    }
  }

  build() {
    PageScaffold({
      title: '风格市场',
      subtitle: '浏览社区风格包，安装到本机',
      actionLabel: '刷新',
      actionSymbol: 'refresh',
      scrollable: false,
      gap: Space.sm,
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
    Column({ space: Space.xs }) {
      SearchField({
        placeholder: '搜索名称、描述或标签',
        value: this.query,
        onInput: (value: string) => {
          this.query = value;
          this.reload();
        }
      })

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

      Segmented({
        items: MARKET_SORTS,
        selected: this.sortIndex(),
        onSelect: (index: number) => {
          this.sort = index === 1 ? 'new' : 'popular';
          this.reload();
        }
      })

      Flex({ wrap: FlexWrap.Wrap, justifyContent: FlexAlign.Start }) {
        Row() {
          TonalButton({
            label: this.loginLabel(),
            symbol: 'person',
            usable: !this.busy,
            onTap: () => {
              this.toggleLogin();
            }
          })
        }
        .padding({ right: Space.xs, bottom: Space.xxs })

        Row() {
          TonalButton({
            label: '刷新列表',
            symbol: 'refresh',
            usable: !this.loading,
            onTap: () => {
              this.reload();
            }
          })
        }
        .padding({ right: Space.xs, bottom: Space.xxs })
      }
      .width('100%')
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
      .padding({ bottom: Space.lg })
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
          .padding({ bottom: Space.lg })
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
              symbol: 'marketplace',
              title: '选一个风格包',
              hint: '右侧显示作者、下载量与安装、点赞操作'
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
    if (this.loading && this.packs.length === 0) {
      Skeleton({ rows: 4 })
    } else if (this.packs.length === 0) {
      EmptyHint({
        symbol: 'marketplace',
        title: this.loading ? '正在加载…' : '还没有可显示的风格包',
        hint: '市场需要网络与 GitHub 账号；未登录或搜索无结果时这里会是空的'
      })
    } else {
      Group() {
        ForEach(this.packs, (pack: MarketplacePack, index: number) => {
          RowItem({
            symbol: 'style',
            accentSymbol: pack.installed === true,
            title: pack.name,
            subtitle: pack.description !== undefined && pack.description.length > 0
              ? pack.description : (pack.author ?? '社区'),
            value: pack.installed === true ? '已安装'
              : (pack.downloads ?? 0).toString() + ' 次下载',
            showChevron: true,
            showDivider: index < this.packs.length - 1,
            onTap: () => {
              this.selectPack(pack.id);
            }
          })
        }, (pack: MarketplacePack) => pack.id)
      }
    }
  }

  @Builder
  detailBody() {
    Column({ space: Space.sm }) {
      Text(this.detailName())
        .fontSize(Type.cardTitle)
        .lineHeight(Leading.cardTitle)
        .fontWeight(FontWeight.Medium)
        .fontColor(Palette.text1())
        .width('100%')

      Row({ space: Space.xs }) {
        Tag({
          text: this.detailInstalled() ? '已安装' : '未安装',
          tone: this.detailInstalled() ? 'success' : 'neutral'
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

      Text(this.detailMeta())
        .fontSize(Type.caption)
        .lineHeight(Leading.caption)
        .fontColor(Palette.text3())
        .width('100%')

      Row({ space: Space.sm }) {
        if (!this.detailInstalled()) {
          PrimaryButton({
            label: '安装到本机',
            usable: true,
            onTap: () => {
              this.install();
            }
          })
        }
        TonalButton({
          label: '点赞',
          symbol: 'sparkles',
          onTap: () => {
            this.like();
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
  sheetBody() {
    Column({ space: Space.md }) {
      if (this.sheetMode === SHEET_DETAIL) {
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
        height: SheetSize.MEDIUM,
        dragBar: true,
        showClose: true,
        onDisappear: () => {
          this.sheetOpen = false;
        }
      })
  }
}
