import { Fmt } from '../common/Format';
import { Labels } from '../common/Labels';
import { Store } from '../common/Store';
import { Layout, Leading, Motion, Palette, Radius, Space, Type } from '../common/Theme';
import {
  Banner,
  EmptyHint,
  Group,
  MetricBlock,
  PageScaffold,
  PrimaryButton,
  RowItem,
  SectionTitle,
  Surface,
  Tag,
  TextButton
} from '../components/Kit';
import { symbolOf } from '../components/Icons';
import { Nav } from '../components/Nav';
import { Api } from '../data/Api';
import { ActivityDay, HistoryEntry, PlatformCapabilities, SettingsSnapshot, UserPreferences } from '../data/Types';

interface DayCell {
  date: string;
  count: number;
}

interface WeekColumn {
  key: string;
  days: DayCell[];
}

const HEAT_LEVELS: number[] = [0, 1, 2, 3, 4];

/**
 * Overview v2: one clear action (dictation), then today's numbers, activity and
 * current configuration. Wide windows put the dictation surface next to the
 * secondary panels; compact windows stack the same blocks.
 */
@Component
export struct OverviewPage {
  @StorageLink('bp') bp: string = 'md';
  @StorageLink('dictPhase') phase: string = 'idle';
  @StorageLink('dictLevel') level: number = 0;
  @StorageLink('dictError') dictError: string = '';
  @StorageLink('backendRevision') revision: number = 0;

  @State private busy: boolean = false;
  @State private error: string = '';
  @State private result: string = '';
  @State private activity: ActivityDay[] = [];
  @State private recent: HistoryEntry[] = [];
  @State private prefs: UserPreferences = {};
  @State private online: boolean = true;
  @State private capabilities: PlatformCapabilities = { platform: 'harmony' };
  @State private configured: boolean = true;
  @State private todayCount: number = 0;
  @State private todayChars: number = 0;
  @State private todayDuration: number = 0;
  @State private weeks: WeekColumn[] = [];

  aboutToAppear(): void {
    Api.takeSplashPlayback();
    this.reload();
  }

  private isWide(): boolean {
    return this.bp === Layout.LG || this.bp === Layout.XL;
  }

  private async reload(): Promise<void> {
    try {
      const activity = await Api.activity();
      this.activity = activity;
      this.computeToday(activity);
      this.weeks = this.buildWeeks(activity);
      const settings: SettingsSnapshot = await Api.settings();
      this.prefs = settings.preferences;
      const history = await Api.history();
      this.recent = history.slice(0, 3);
      const network = await Api.network();
      this.online = network.online;
      this.capabilities = await Api.platformCapabilities();
      try {
        const credentials = await Api.credentialStatus();
        this.configured = credentials.asrConfigured === true;
      } catch (error) {
        this.configured = false;
      }
      this.error = '';
    } catch (error) {
      this.error = (error as Error).message;
    }
  }

  private computeToday(activity: ActivityDay[]): void {
    const today: string = Fmt.todayIso();
    let count: number = 0;
    let chars: number = 0;
    let duration: number = 0;
    for (let index = 0; index < activity.length; index++) {
      if (activity[index].date === today) {
        count = activity[index].count;
        chars = activity[index].chars ?? 0;
        duration = activity[index].durationMs ?? 0;
      }
    }
    this.todayCount = count;
    this.todayChars = chars;
    this.todayDuration = duration;
  }

  /** Twelve weeks, Sunday-first like the HarmonyOS calendar. */
  private buildWeeks(activity: ActivityDay[]): WeekColumn[] {
    const counts: Map<string, number> = new Map<string, number>();
    for (let index = 0; index < activity.length; index++) {
      counts.set(activity[index].date, activity[index].count);
    }
    const today: Date = new Date();
    const endOfWeek: Date = new Date(today.getFullYear(), today.getMonth(), today.getDate());
    const columns: WeekColumn[] = [];
    for (let week = 11; week >= 0; week--) {
      const days: DayCell[] = [];
      for (let day = 0; day < 7; day++) {
        const cell: Date = new Date(endOfWeek.getFullYear(), endOfWeek.getMonth(),
          endOfWeek.getDate() - (week * 7) - (6 - day));
        const iso: string = cell.getFullYear().toString() + '-' +
          (cell.getMonth() + 1 < 10 ? '0' : '') + (cell.getMonth() + 1).toString() + '-' +
          (cell.getDate() < 10 ? '0' : '') + cell.getDate().toString();
        days.push({ date: iso, count: counts.get(iso) ?? 0 });
      }
      columns.push({ key: 'w' + week.toString(), days: days });
    }
    return columns;
  }

  private heatLevel(count: number): number {
    if (count <= 0) return 0;
    if (count <= 2) return 1;
    if (count <= 5) return 2;
    if (count <= 9) return 3;
    return 4;
  }

  private greeting(): string {
    const hour: number = new Date().getHours();
    if (hour < 6) return '夜深了';
    if (hour < 12) return '早上好';
    if (hour < 18) return '下午好';
    return '晚上好';
  }

  private configuredLabel(): string {
    if (this.configured) return this.greeting() + ' · 说一句就上屏';
    return this.greeting() + ' · 还差一个转写密钥';
  }

  private microphoneLabel(): string {
    const name: string | undefined = this.prefs.microphoneDeviceName;
    if (name === undefined || name.length === 0) return '系统默认';
    return name;
  }

  private phaseTitle(): string {
    if (this.phase === 'starting') return '正在准备麦克风';
    if (this.phase === 'recording') return '正在聆听';
    if (this.phase === 'processing') return '正在转写与润色';
    if (this.phase === 'cancelling') return '正在取消';
    if (this.dictError.length > 0) return '上次录音失败';
    return '点一下，开始说话';
  }

  private phaseHint(): string {
    if (this.dictError.length > 0) return this.dictError;
    if (this.phase === 'recording') return '再次点击结束并上屏，说完直接停也可以';
    if (this.phase === 'idle') return '录音在本机完成，只有转写请求会上传音频';
    return '';
  }

  private async toggleDictation(): Promise<void> {
    if (this.busy) return;
    this.busy = true;
    this.error = '';
    try {
      if (this.phase === 'idle' || this.dictError.length > 0) {
        this.result = '';
        await Api.startDictation();
      } else {
        const voice = await Api.stopDictation();
        this.result = voice.polishedText.length > 0 ? voice.polishedText : voice.rawText;
        await this.reload();
      }
    } catch (error) {
      this.error = (error as Error).message;
    } finally {
      this.busy = false;
    }
  }

  private async cancelDictation(): Promise<void> {
    try {
      await Api.cancelDictation();
    } catch (error) {
      this.error = (error as Error).message;
    }
  }

  build() {
    PageScaffold({
      title: '概览',
      subtitle: this.configuredLabel(),
      actionLabel: '刷新',
      actionSymbol: 'refresh',
      onAction: () => {
        this.reload();
      }
    }) {
      if (!this.configured) {
        this.setupBanner()
      }
      if (this.isWide()) {
        Row({ space: Space.md }) {
          Column({ space: Space.md }) {
            this.dictationCard()
            this.resultCard()
          }
          .layoutWeight(5)
          .alignItems(HorizontalAlign.Start)

          Column({ space: Space.md }) {
            this.todayCard()
            this.activityCard()
            this.configCard()
            this.recentCard()
          }
          .layoutWeight(4)
          .alignItems(HorizontalAlign.Start)
        }
        .width('100%')
        .alignItems(VerticalAlign.Top)
      } else {
        Column({ space: Space.md }) {
          this.dictationCard()
          this.resultCard()
          this.todayCard()
          this.activityCard()
          this.configCard()
          this.recentCard()
        }
        .width('100%')
      }
    }
  }

  @Builder
  setupBanner() {
    Banner({
      title: '先配一个转写密钥',
      text: '鸿蒙端把转写交给云端引擎，密钥保存在系统密钥库。没有密钥时录音会失败，历史里只会留下错误码。',
      action: '去配凭据',
      actionSymbol: 'settings',
      onAction: () => Nav.go(Nav.CREDENTIALS)
    })
  }

  /** One tap target for the whole dictation loop; no second, competing button. */
  @Builder
  dictationCard() {
    Surface({ inset: Space.md }) {
      Column({ space: Space.md }) {
        Stack() {
          Column()
            .width(168)
            .height(168)
            .borderRadius(Radius.pill)
            .backgroundColor(Palette.accentSoft())
            .opacity(0.5)
          Column()
            .width(136)
            .height(136)
            .borderRadius(Radius.pill)
            .backgroundColor(Palette.accentSoft())
            .opacity(this.phase === 'recording' ? 0.55 + this.level * 0.45 : 1)
            .scale({ x: 1 + this.level * 0.06, y: 1 + this.level * 0.06 })
            .animation({ duration: Motion.quick, curve: Curve.EaseOut })
          Column()
            .width(104)
            .height(104)
            .borderRadius(Radius.pill)
            .backgroundColor(this.phase === 'recording' ? Palette.accentPressed() : Palette.accent())
            .justifyContent(FlexAlign.Center)
          SymbolGlyph(symbolOf(this.phase === 'recording' ? 'stop' : 'mic'))
            .fontSize(38)
            .fontColor([Palette.onAccent()])
        }
        .width(168)
        .height(168)
        .onClick(() => {
          this.toggleDictation();
        })

        Column({ space: Space.xxs }) {
          Text(this.phaseTitle())
            .fontSize(Type.section)
            .lineHeight(Leading.section)
            .fontWeight(FontWeight.Medium)
            .fontColor(Palette.text1())
          if (this.phaseHint().length > 0) {
            Text(this.phaseHint())
              .fontSize(Type.label)
              .lineHeight(Leading.label)
              .fontColor(this.dictError.length > 0 ? Palette.danger() : Palette.text3())
              .textAlign(TextAlign.Center)
          }
          if (this.error.length > 0) {
            Text(this.error)
              .fontSize(Type.label)
              .lineHeight(Leading.label)
              .fontColor(Palette.danger())
              .textAlign(TextAlign.Center)
          }
        }
        .width('100%')
        .alignItems(HorizontalAlign.Center)

        Row({ space: Space.sm }) {
          if (this.phase === 'recording') {
            PrimaryButton({
              label: '结束并上屏',
              onTap: () => {
                this.toggleDictation();
              }
            })
            TextButton({
              label: '取消',
              tone: 'quiet',
              onTap: () => {
                this.cancelDictation();
              }
            })
          } else {
            PrimaryButton({
              label: '开始说话',
              usable: !this.busy,
              onTap: () => {
                this.toggleDictation();
              }
            })
          }
        }
        .width('100%')
        .justifyContent(FlexAlign.Center)

        Row({ space: Space.xs }) {
          Tag({ text: this.prefs.polishMode === 'raw' ? '原文模式' : '润色模式', tone: 'accent' })
          Tag({ text: Labels.provider(this.prefs.activeAsrProvider), tone: 'neutral' })
          Tag({ text: this.online ? '在线' : '离线', tone: this.online ? 'success' : 'warning' })
        }
        .width('100%')
        .justifyContent(FlexAlign.Center)
      }
      .width('100%')
    }
  }

  @Builder
  resultCard() {
    if (this.result.length > 0) {
      Surface({ inset: Space.md }) {
        Column({ space: Space.xs }) {
          Row({ space: Space.sm }) {
            Text('刚刚上屏')
              .fontSize(Type.label)
              .lineHeight(Leading.label)
              .fontWeight(FontWeight.Medium)
              .fontColor(Palette.text2())
              .layoutWeight(1)
            TextButton({
              label: '复制',
              symbol: 'copy',
              onTap: () => {
                Api.copyText(this.result);
              }
            })
          }
          .width('100%')

          Text(this.result)
            .fontSize(Type.body)
            .lineHeight(Leading.body)
            .fontColor(Palette.text1())
            .width('100%')
            .copyOption(CopyOptions.LocalDevice)
        }
        .width('100%')
      }
    }
  }

  @Builder
  todayCard() {
    Surface({ inset: Space.md }) {
      Column({ space: Space.sm }) {
        SectionTitle({ title: '今天', hint: '统计只留在这台设备上' })
        Row({ space: Space.sm }) {
          MetricBlock({ label: '段数', value: this.todayCount.toString() })
          MetricBlock({ label: '字数', value: Fmt.count(this.todayChars) })
          MetricBlock({ label: '时长', value: Fmt.duration(this.todayDuration) })
        }
        .width('100%')
        .alignItems(VerticalAlign.Bottom)
      }
      .width('100%')
    }
  }

  @Builder
  activityCard() {
    Surface({ inset: Space.md }) {
      Column({ space: Space.sm }) {
        SectionTitle({ title: '近 12 周', hint: '一格一天，越深说得越多' })
        Row() {
          ForEach(this.weeks, (week: WeekColumn) => {
            Column({ space: 4 }) {
              ForEach(week.days, (day: DayCell) => {
                Column()
                  .width(13)
                  .height(13)
                  .backgroundColor(Palette.heat(this.heatLevel(day.count)))
                  .borderRadius(3)
              }, (day: DayCell) => day.date)
            }
          }, (week: WeekColumn) => week.key)
        }
        .width('100%')
        .justifyContent(FlexAlign.SpaceBetween)

        Row({ space: Space.xxs }) {
          Text('少')
            .fontSize(Type.caption)
            .fontColor(Palette.text3())
          ForEach(HEAT_LEVELS, (level: number) => {
            Column()
              .width(10)
              .height(10)
              .backgroundColor(Palette.heat(level))
              .borderRadius(2)
          }, (level: number) => 'legend-' + level.toString())
          Text('多')
            .fontSize(Type.caption)
            .fontColor(Palette.text3())
        }
      }
      .width('100%')
    }
  }

  @Builder
  configCard() {
    Group() {
      Column() {
        Row() {
          SectionTitle({ title: '当前配置' })
        }
        .padding({ left: Space.md, right: Space.md, top: Space.sm, bottom: Space.xxs })
          .width('100%')

        RowItem({
          title: '转写引擎',
          value: Labels.provider(this.prefs.activeAsrProvider),
          showChevron: true,
          showDivider: true,
          onTap: () => Nav.go(Nav.SETTINGS)
        })
        RowItem({
          title: '润色模型',
          value: Labels.provider(this.prefs.activeLlmProvider),
          showChevron: true,
          showDivider: true,
          onTap: () => Nav.go(Nav.SETTINGS)
        })
        RowItem({
          title: '麦克风',
          value: this.microphoneLabel(),
          showChevron: true,
          showDivider: true,
          onTap: () => Nav.go(Nav.SETTINGS)
        })
        RowItem({
          title: '网络',
          value: this.online ? '已连接' : '未连接',
          showChevron: true,
          showDivider: true,
          onTap: () => Nav.go(Nav.SETTINGS)
        })
        RowItem({
          title: '离线听写',
          value: this.capabilities.supportsLocalAsr === true ? '可用' : '需联网转写',
          showChevron: true,
          showDivider: false,
          onTap: () => Nav.go(Nav.LOCALASR)
        })
      }
      .width('100%')
    }
  }

  @Builder
  recentCard() {
    Group() {
      Column() {
        Row() {
          SectionTitle({
            title: '最近',
            action: '全部历史',
            onAction: () => Nav.go(Nav.HISTORY)
          })
        }
        .padding({ left: Space.md, right: Space.md, top: Space.sm, bottom: Space.xxs })
        .width('100%')

        if (this.recent.length === 0) {
          EmptyHint({ symbol: 'history', title: '还没有听写记录', hint: '点上面的麦克风说第一句话' })
        } else {
          ForEach(this.recent, (entry: HistoryEntry, index: number) => {
            RowItem({
              title: Fmt.preview(entry.finalText, 40),
              subtitle: Fmt.relative(entry.createdAt),
              showChevron: true,
              showDivider: index < this.recent.length - 1,
              onTap: () => {
                Nav.go(Nav.HISTORY);
                Store.put<string>('detailId', entry.id);
              }
            })
          }, (entry: HistoryEntry) => entry.id)
        }
      }
      .width('100%')
    }
  }
}
