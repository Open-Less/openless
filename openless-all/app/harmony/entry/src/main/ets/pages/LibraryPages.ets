import { Fmt } from '../common/Format';
import { C, R, S, T } from '../common/Tokens';
import { Card, EmptyState, GhostButton, PageTitle, Pill, SearchField, SectionHeader } from '../components/Common';
import { symbolOf } from '../components/Icons';
import { Api } from '../data/Api';
import { CorrectionRule, DictionaryEntry, MarketplacePack, StylePack } from '../data/Types';

/**
 * 词典 / 纠正规则 / 风格 / 风格市场.
 *
 * Four list-shaped surfaces that all follow the same pattern: a title block, an
 * optional composer, then a card per entry with a toggle and a destructive
 * action. They stay one column at every breakpoint because the rows are already
 * dense; only the gutter changes.
 */
@Component
export struct VocabPage {
  @StorageLink('bp') bp: string = 'md';

  @State private entries: DictionaryEntry[] = [];
  @State private draft: string = '';
  @State private query: string = '';
  @State private error: string = '';
  @State private busy: boolean = false;

  aboutToAppear(): void {
    this.reload();
  }

  private async reload(): Promise<void> {
    try {
      this.entries = await Api.vocabulary();
      this.error = '';
    } catch (error) {
      this.error = (error as Error).message;
    }
  }

  private async add(): Promise<void> {
    const phrase: string = this.draft.trim();
    if (phrase.length === 0) return;
    this.busy = true;
    try {
      await Api.addVocabulary(phrase);
      this.draft = '';
      await this.reload();
    } catch (error) {
      this.error = (error as Error).message;
    } finally {
      this.busy = false;
    }
  }

  private filtered(): DictionaryEntry[] {
    if (this.query.length === 0) return this.entries;
    const needle: string = this.query.toLowerCase();
    const result: DictionaryEntry[] = [];
    for (let index = 0; index < this.entries.length; index++) {
      if (this.entries[index].phrase.toLowerCase().indexOf(needle) >= 0) result.push(this.entries[index]);
    }
    return result;
  }

  build() {
    Column() {
      PageTitle({ title: '词典', subtitle: this.entries.length.toString() + ' 个词条 · 转写时优先命中' })

      Row({ space: S.sm }) {
        TextInput({ placeholder: '添加词条，例如人名、产品名', text: this.draft })
          .fontSize(T.body)
          .layoutWeight(1)
          .height(44)
          .backgroundColor(C.sunken())
          .borderRadius(R.md)
          .onChange((value: string) => {
            this.draft = value;
          })
          .onSubmit(() => {
            this.add();
          })
        GhostButton({
          label: this.busy ? '添加中' : '添加',
          symbol: 'add',
          usable: !this.busy && this.draft.trim().length > 0,
          onTap: () => {
            this.add();
          }
        })
      }
      .width('100%')
      .padding({ bottom: S.sm })

      SearchField({
        placeholder: '搜索词条',
        value: this.query,
        onInput: (value: string) => {
          this.query = value;
        }
      })

      if (this.error.length > 0) {
        Text(this.error).fontSize(T.footnote).fontColor(C.danger()).padding({ top: S.sm })
      }

      Scroll() {
        Column({ space: S.sm }) {
          if (this.filtered().length === 0) {
            EmptyState({
              symbol: 'vocab',
              title: this.query.length > 0 ? '没有匹配的词条' : '词典还是空的',
              hint: '把常被听错的名字加进来，转写会优先匹配'
            })
          } else {
            ForEach(this.filtered(), (entry: DictionaryEntry) => {
              Row({ space: S.md }) {
                Column({ space: S.xxs }) {
                  Text(entry.phrase)
                    .fontSize(T.body)
                    .fontColor(C.textPrimary())
                  Text((entry.hits ?? 0).toString() + ' 次命中' +
                    (entry.note !== undefined && entry.note !== null && entry.note.length > 0 ? ' · ' + entry.note : ''))
                    .fontSize(T.caption)
                    .fontColor(C.textTertiary())
                }
                .alignItems(HorizontalAlign.Start)
                .layoutWeight(1)

                Toggle({ type: ToggleType.Switch, isOn: entry.enabled })
                  .selectedColor(C.accent())
                  .onChange((value: boolean) => {
                    Api.setVocabularyEnabled(entry.id, value).then(() => this.reload()).catch((error: Error) => {
                      this.error = error.message;
                    });
                  })
                SymbolGlyph(symbolOf('trash'))
                  .fontSize(18)
                  .fontColor([C.textTertiary()])
                  .onClick(() => {
                    Api.removeVocabulary(entry.id).then(() => this.reload()).catch((error: Error) => {
                      this.error = error.message;
                    });
                  })
              }
              .width('100%')
              .padding(S.md)
              .backgroundColor(C.surface())
              .borderRadius(R.md)
            }, (entry: DictionaryEntry) => entry.id)
          }
        }
        .width('100%')
        .padding({ top: S.sm, bottom: S.xxl })
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
export struct CorrectionsPage {
  @StorageLink('bp') bp: string = 'md';

  @State private rules: CorrectionRule[] = [];
  @State private pattern: string = '';
  @State private replacement: string = '';
  @State private error: string = '';
  @State private busy: boolean = false;

  aboutToAppear(): void {
    this.reload();
  }

  private async reload(): Promise<void> {
    try {
      this.rules = await Api.correctionRules();
      this.error = '';
    } catch (error) {
      this.error = (error as Error).message;
    }
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
      await this.reload();
    } catch (error) {
      this.error = (error as Error).message;
    } finally {
      this.busy = false;
    }
  }

  build() {
    Column() {
      PageTitle({ title: '纠正规则', subtitle: this.rules.length.toString() + ' 条规则 · 在润色前替换' })

      Card() {
        Column({ space: S.sm }) {
          SectionHeader({ title: '新建规则', hint: '把「听错的样子」替换成「正确的写法」' })
          TextInput({ placeholder: '错误的写法', text: this.pattern })
            .fontSize(T.body)
            .height(44)
            .backgroundColor(C.sunken())
            .borderRadius(R.md)
            .onChange((value: string) => {
              this.pattern = value;
            })
          TextInput({ placeholder: '正确的写法', text: this.replacement })
            .fontSize(T.body)
            .height(44)
            .backgroundColor(C.sunken())
            .borderRadius(R.md)
            .onChange((value: string) => {
              this.replacement = value;
            })
          Row() {
            GhostButton({
              label: this.busy ? '保存中' : '添加规则',
              symbol: 'add',
              usable: !this.busy && this.pattern.trim().length > 0,
              onTap: () => {
                this.add();
              }
            })
          }
          .width('100%')
          .justifyContent(FlexAlign.End)
        }
        .width('100%')
      }

      if (this.error.length > 0) {
        Text(this.error).fontSize(T.footnote).fontColor(C.danger()).padding({ top: S.sm })
      }

      Scroll() {
        Column({ space: S.sm }) {
          if (this.rules.length === 0) {
            EmptyState({ symbol: 'corrections', title: '还没有规则', hint: '规则越具体越好，避免误伤正常文本' })
          } else {
            ForEach(this.rules, (rule: CorrectionRule) => {
              Row({ space: S.md }) {
                Column({ space: S.xs }) {
                  Row({ space: S.xs }) {
                    Text(rule.pattern)
                      .fontSize(T.callout)
                      .fontColor(C.textSecondary())
                    Text('→')
                      .fontSize(T.caption)
                      .fontColor(C.textTertiary())
                    Text(rule.replacement)
                      .fontSize(T.callout)
                      .fontColor(C.textPrimary())
                  }
                  Row({ space: S.xs }) {
                    if (rule.source === 'learned') {
                      Pill({ text: '自动学习', tone: 'accent' })
                    }
                    Text(Fmt.relative(rule.createdAt ?? ''))
                      .fontSize(T.caption)
                      .fontColor(C.textTertiary())
                  }
                }
                .alignItems(HorizontalAlign.Start)
                .layoutWeight(1)

                Toggle({ type: ToggleType.Switch, isOn: rule.enabled })
                  .selectedColor(C.accent())
                  .onChange((value: boolean) => {
                    Api.setCorrectionRuleEnabled(rule.id, value).then(() => this.reload()).catch((error: Error) => {
                      this.error = error.message;
                    });
                  })
                SymbolGlyph(symbolOf('trash'))
                  .fontSize(18)
                  .fontColor([C.textTertiary()])
                  .onClick(() => {
                    Api.removeCorrectionRule(rule.id).then(() => this.reload()).catch((error: Error) => {
                      this.error = error.message;
                    });
                  })
              }
              .width('100%')
              .padding(S.md)
              .backgroundColor(C.surface())
              .borderRadius(R.md)
            }, (rule: CorrectionRule) => rule.id)
          }
        }
        .width('100%')
        .padding({ top: S.sm, bottom: S.xxl })
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
export struct StylePage {
  @StorageLink('bp') bp: string = 'md';
  @StorageLink('backendRevision') revision: number = 0;

  @State private packs: StylePack[] = [];
  @State private active: string = '';
  @State private error: string = '';

  aboutToAppear(): void {
    this.reload();
  }

  private async reload(): Promise<void> {
    try {
      this.packs = await Api.stylePacks();
      const snapshot = await Api.settings();
      this.active = snapshot.preferences.activeStylePackId ?? '';
      this.error = '';
    } catch (error) {
      this.error = (error as Error).message;
    }
  }

  build() {
    Column() {
      PageTitle({ title: '风格', subtitle: this.packs.length.toString() + ' 个风格包 · 决定润色的语气' })

      if (this.error.length > 0) {
        Text(this.error).fontSize(T.footnote).fontColor(C.danger()).padding({ bottom: S.sm })
      }

      Scroll() {
        Column({ space: S.sm }) {
          if (this.packs.length === 0) {
            EmptyState({ symbol: 'style', title: '还没有风格包', hint: '去风格市场安装一个，或创建自定义风格' })
          } else {
            ForEach(this.packs, (pack: StylePack) => {
              Column({ space: S.sm }) {
                Row({ space: S.sm }) {
                  Text(pack.name)
                    .fontSize(T.body)
                    .fontWeight(FontWeight.Medium)
                    .fontColor(C.textPrimary())
                    .layoutWeight(1)
                  if (pack.id === this.active) {
                    Pill({ text: '使用中', tone: 'accent' })
                  } else if (pack.builtIn === true) {
                    Pill({ text: '内置', tone: 'neutral' })
                  }
                  Toggle({ type: ToggleType.Switch, isOn: pack.enabled !== false })
                    .selectedColor(C.accent())
                    .onChange((value: boolean) => {
                      Api.setStylePackEnabled(pack.id, value).then(() => this.reload()).catch((error: Error) => {
                        this.error = error.message;
                      });
                    })
                }
                .width('100%')

                if (pack.description !== undefined && pack.description.length > 0) {
                  Text(pack.description)
                    .fontSize(T.footnote)
                    .fontColor(C.textSecondary())
                    .width('100%')
                }

                if (pack.id !== this.active) {
                  Row() {
                    GhostButton({
                      label: '设为当前风格',
                      onTap: () => {
                        Api.activateStylePack(pack.id).then(() => this.reload()).catch((error: Error) => {
                          this.error = error.message;
                        });
                      }
                    })
                  }
                  .width('100%')
                  .justifyContent(FlexAlign.End)
                }
              }
              .width('100%')
              .alignItems(HorizontalAlign.Start)
              .padding(S.md)
              .backgroundColor(C.surface())
              .borderRadius(R.md)
            }, (pack: StylePack) => pack.id)
          }
        }
        .width('100%')
        .padding({ top: S.sm, bottom: S.xxl })
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
export struct MarketplacePage {
  @StorageLink('bp') bp: string = 'md';

  @State private packs: MarketplacePack[] = [];
  @State private query: string = '';
  @State private error: string = '';
  @State private busy: boolean = false;

  aboutToAppear(): void {
    this.reload();
  }

  private async reload(): Promise<void> {
    this.busy = true;
    try {
      this.packs = await Api.marketplace(this.query);
      this.error = '';
    } catch (error) {
      this.error = (error as Error).message;
    } finally {
      this.busy = false;
    }
  }

  build() {
    Column() {
      PageTitle({ title: '风格市场', subtitle: '浏览社区风格包，安装到本机' })

      Row({ space: S.sm }) {
        SearchField({
          placeholder: '搜索名称、描述或标签',
          value: this.query,
          onInput: (value: string) => {
            this.query = value;
          }
        })
        GhostButton({
          label: this.busy ? '刷新中' : '刷新',
          symbol: 'refresh',
          usable: !this.busy,
          onTap: () => {
            this.reload();
          }
        })
      }
      .width('100%')

      if (this.error.length > 0) {
        Text(this.error)
          .fontSize(T.footnote)
          .fontColor(C.danger())
          .padding(S.sm)
          .width('100%')
          .backgroundColor(C.dangerSoft())
          .borderRadius(R.sm)
          .margin({ top: S.sm })
      }

      Scroll() {
        Column({ space: S.sm }) {
          if (this.packs.length === 0) {
            EmptyState({
              symbol: 'marketplace',
              title: this.busy ? '正在加载…' : '还没有可显示的风格包',
              hint: '市场需要网络与 GitHub 账号；未登录时这里会是空的'
            })
          } else {
            ForEach(this.packs, (pack: MarketplacePack) => {
              Column({ space: S.xs }) {
                Row({ space: S.sm }) {
                  Text(pack.name)
                    .fontSize(T.body)
                    .fontWeight(FontWeight.Medium)
                    .fontColor(C.textPrimary())
                    .layoutWeight(1)
                  if (pack.installed === true) {
                    Pill({ text: '已安装', tone: 'success' })
                  } else {
                    GhostButton({
                      label: '安装',
                      onTap: () => {
                        Api.installMarketplacePack(pack.id).then(() => this.reload()).catch((error: Error) => {
                          this.error = error.message;
                        });
                      }
                    })
                  }
                }
                .width('100%')

                if (pack.description !== undefined && pack.description.length > 0) {
                  Text(pack.description)
                    .fontSize(T.footnote)
                    .fontColor(C.textSecondary())
                    .width('100%')
                }
                Text((pack.author ?? '社区') + ' · ' + (pack.downloads ?? 0).toString() + ' 次下载')
                  .fontSize(T.caption)
                  .fontColor(C.textTertiary())
              }
              .width('100%')
              .alignItems(HorizontalAlign.Start)
              .padding(S.md)
              .backgroundColor(C.surface())
              .borderRadius(R.md)
            }, (pack: MarketplacePack) => pack.id)
          }
        }
        .width('100%')
        .padding({ top: S.sm, bottom: S.xxl })
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
