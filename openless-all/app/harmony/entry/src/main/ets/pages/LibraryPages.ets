import { Fmt } from '../common/Format';
import { C, R, S, T } from '../common/Tokens';
import { Card, EmptyState, GhostButton, PageTitle, Pill, SearchField, SectionHeader } from '../components/Common';
import { symbolOf } from '../components/Icons';
import { Nav } from '../components/Nav';
import { Api, PendingCorrection } from '../data/Api';
import { CorrectionRule, DictionaryEntry, MarketplacePack, StylePack } from '../data/Types';
import { StylePackPayload } from '../data/Api';

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
      Row({ space: S.sm }) {
        Column() {
          PageTitle({ title: '词典', subtitle: this.entries.length.toString() + ' 个词条 · 转写时优先命中' })
        }
        .layoutWeight(1)
        GhostButton({
          label: '纠正规则',
          symbol: 'corrections',
          onTap: () => Nav.go(Nav.CORRECTIONS)
        })
      }
      .width('100%')
      .alignItems(VerticalAlign.Center)

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
  @State private pending: PendingCorrection[] = [];
  @State private error: string = '';
  @State private busy: boolean = false;

  aboutToAppear(): void {
    this.reload();
  }

  private async reload(): Promise<void> {
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

      if (this.pending.length > 0) {
        Card() {
          Column({ space: S.xs }) {
            SectionHeader({ title: '学习建议', hint: '模型从历史里发现的重复纠正，接受后成为规则' })
            ForEach(this.pending, (item: PendingCorrection) => {
              Row({ space: S.sm }) {
                Column({ space: S.xxs }) {
                  Text(item.pattern + ' → ' + item.replacement)
                    .fontSize(T.callout)
                    .fontColor(C.textPrimary())
                }
                .alignItems(HorizontalAlign.Start)
                .layoutWeight(1)
                GhostButton({
                  label: '接受',
                  symbol: 'check',
                  onTap: () => {
                    Api.acceptCorrection(item.id).then(() => this.reload()).catch((error: Error) => {
                      this.error = error.message;
                    });
                  }
                })
                GhostButton({
                  label: '忽略',
                  onTap: () => {
                    Api.rejectCorrection(item.id).then(() => this.reload()).catch((error: Error) => {
                      this.error = error.message;
                    });
                  }
                })
              }
              .width('100%')
              .padding({ top: S.xxs, bottom: S.xxs })
            }, (item: PendingCorrection) => item.id)
          }
          .width('100%')
        }
      }

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
  @State private editing: string = '';
  @State private draftPrompt: string = '';
  @State private newName: string = '';
  @State private newPrompt: string = '';
  @State private composerOpen: boolean = false;

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

  private async savePrompt(pack: StylePack): Promise<void> {
    try {
      await Api.saveStylePack({
        id: pack.id,
        name: pack.name,
        description: pack.description ?? '',
        systemPrompt: this.draftPrompt,
        enabled: pack.enabled !== false
      } as StylePackPayload);
      this.editing = '';
      await this.reload();
    } catch (error) {
      this.error = (error as Error).message;
    }
  }

  private async createPack(): Promise<void> {
    const name: string = this.newName.trim();
    if (name.length === 0) return;
    try {
      await Api.saveStylePack({
        name: name,
        description: '',
        systemPrompt: this.newPrompt.trim(),
        enabled: true
      } as StylePackPayload);
      this.newName = '';
      this.newPrompt = '';
      this.composerOpen = false;
      await this.reload();
    } catch (error) {
      this.error = (error as Error).message;
    }
  }

  private async removePack(id: string): Promise<void> {
    try {
      await Api.deleteStylePack(id);
      await this.reload();
    } catch (error) {
      this.error = (error as Error).message;
    }
  }

  build() {
    Column() {
      PageTitle({ title: '风格', subtitle: this.packs.length.toString() + ' 个风格包 · 决定润色的语气' })

      Row({ space: S.sm }) {
        GhostButton({
          label: '风格市场',
          symbol: 'marketplace',
          onTap: () => Nav.go(Nav.MARKETPLACE)
        })
        GhostButton({
          label: this.composerOpen ? '收起' : '新建风格',
          symbol: 'add',
          onTap: () => {
            this.composerOpen = !this.composerOpen;
          }
        })
      }
      .width('100%')
      .padding({ bottom: S.sm })

      if (this.composerOpen) {
        Card() {
          Column({ space: S.sm }) {
            SectionHeader({ title: '新建风格', hint: '提示词决定润色的角色与语气' })
            TextInput({ placeholder: '名称，例如「工作邮件」', text: this.newName })
              .fontSize(T.body)
              .height(44)
              .backgroundColor(C.sunken())
              .borderRadius(R.md)
              .onChange((value: string) => {
                this.newName = value;
              })
            TextArea({ placeholder: '系统提示词', text: this.newPrompt })
              .fontSize(T.body)
              .height(120)
              .backgroundColor(C.sunken())
              .borderRadius(R.md)
              .padding(S.sm)
              .onChange((value: string) => {
                this.newPrompt = value;
              })
            Row() {
              GhostButton({
                label: '保存风格',
                symbol: 'check',
                usable: this.newName.trim().length > 0,
                onTap: () => {
                  this.createPack();
                }
              })
            }
            .width('100%')
            .justifyContent(FlexAlign.End)
          }
          .width('100%')
        }
      }

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

                if (this.editing === pack.id) {
                  TextArea({ placeholder: '系统提示词', text: this.draftPrompt })
                    .fontSize(T.footnote)
                    .height(180)
                    .backgroundColor(C.sunken())
                    .borderRadius(R.sm)
                    .padding(S.sm)
                    .onChange((value: string) => {
                      this.draftPrompt = value;
                    })
                  Row({ space: S.sm }) {
                    GhostButton({
                      label: '保存',
                      symbol: 'check',
                      onTap: () => {
                        this.savePrompt(pack);
                      }
                    })
                    GhostButton({
                      label: '取消',
                      onTap: () => {
                        this.editing = '';
                      }
                    })
                  }
                  .width('100%')
                  .justifyContent(FlexAlign.End)
                } else {
                  Row({ space: S.sm }) {
                    GhostButton({
                      label: '编辑提示词',
                      onTap: () => {
                        this.editing = pack.id;
                        this.draftPrompt = pack.systemPrompt ?? '';
                      }
                    })
                    GhostButton({
                      label: '删除',
                      symbol: 'trash',
                      onTap: () => {
                        this.removePack(pack.id);
                      }
                    })
                  }
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
  @State private sort: string = 'popular';
  @State private loginCode: string = '';
  @State private loginState: string = '';

  aboutToAppear(): void {
    this.reload();
  }

  private async reload(): Promise<void> {
    this.busy = true;
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
      this.busy = false;
    }
  }

  private async startLogin(): Promise<void> {
    try {
      const flow = await Api.githubDeviceFlowStart();
      this.loginCode = flow.userCode;
      this.loginState = '等待授权…';
      Api.copyText(flow.userCode);
      let attempts = 0;
      while (attempts < 60) {
        attempts++;
        await new Promise<void>((resolve) => setTimeout(() => resolve(), Math.max(2, flow.interval) * 1000));
        const poll = await Api.githubDeviceFlowPoll(flow.flowId);
        if (poll.state === 'authorized') {
          this.loginState = '已登录 ' + (poll.login ?? '');
          this.loginCode = '';
          await this.reload();
          return;
        }
        if (poll.state === 'expired' || poll.state === 'error') {
          this.loginState = '登录失败';
          this.loginCode = '';
          return;
        }
      }
      this.loginState = '登录超时';
      this.loginCode = '';
    } catch (error) {
      this.loginState = '登录不可用';
      this.error = (error as Error).message;
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

      Row({ space: S.sm }) {
        GhostButton({
          label: this.loginState.length > 0 ? this.loginState : 'GitHub 登录',
          symbol: 'person',
          onTap: () => {
            this.startLogin();
          }
        })
        if (this.loginCode.length > 0) {
          Text('在浏览器输入 ' + this.loginCode)
            .fontSize(T.caption)
            .fontColor(C.accent())
        }
      }
      .width('100%')
      .padding({ top: S.sm })

      Row({ space: S.sm }) {
        ForEach(['popular', 'new'], (option: string) => {
          Text(option === 'popular' ? '按热度' : '最新')
            .fontSize(T.caption)
            .fontColor(this.sort === option ? C.textOnAccent() : C.textSecondary())
            .padding({ left: S.md, right: S.md, top: S.xxs, bottom: S.xxs })
            .backgroundColor(this.sort === option ? C.accent() : C.sunken())
            .borderRadius(R.pill)
            .onClick(() => {
              this.sort = option;
              this.reload();
            })
        }, (option: string) => 'sort-' + option)
      }
      .width('100%')
      .padding({ top: S.sm })

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
                Row({ space: S.sm }) {
                  Text((pack.author ?? '社区') + ' · ' + (pack.downloads ?? 0).toString() + ' 次下载 · ' +
                    (pack.likes ?? 0).toString() + ' 赞')
                    .fontSize(T.caption)
                    .fontColor(C.textTertiary())
                    .layoutWeight(1)
                  GhostButton({
                    label: '点赞',
                    onTap: () => {
                      Api.likePack(pack.id).then(() => this.reload()).catch((error: Error) => {
                        this.error = error.message;
                      });
                    }
                  })
                }
                .width('100%')
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
