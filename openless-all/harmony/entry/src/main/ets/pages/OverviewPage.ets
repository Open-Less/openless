import { common } from '@kit.AbilityKit';
import { Dashboard, DashboardCard } from '../common/Dashboard';
import { Fmt } from '../common/Format';
import { Labels } from '../common/Labels';
import { Store } from '../common/Store';
import { Layout, Leading, Palette, Radius, Size, Space, Type } from '../common/Theme';
import {
  Banner,
  EmptyHint,
  Group,
  MetricBlock,
  PageScaffold,
  RowItem,
  SectionTitle,
  Segmented,
  SettingSwitch,
  SheetTitle,
  Surface,
  Tag,
  TextButton,
  TonalButton,
  notify
} from '../components/Kit';
import { Nav } from '../components/Nav';
import { VoiceControl } from '../components/VoiceControl';
import { Api } from '../data/Api';
import { ActivityDay, HistoryEntry, PlatformCapabilities, SettingsSnapshot, UserPreferences } from '../data/Types';

/** One day of the 12-week heat map. */
interface DayCell {
  date: string;
  count: number;
}

interface WeekColumn {
  key: string;
  days: DayCell[];
}

/** One bar of the 7/30-day trend. */
interface ActivityBucket {
  date: string;
  value: number;
}

/** One grid row: two or three card ids that share a line. */
interface CardRow {
  key: string;
  ids: string[];
}

const HEAT_LEVELS: number[] = [0, 1, 2, 3, 4];
const PERIOD_LABELS: string[] = ['近 7 天', '近 30 天'];
const METRIC_LABELS: string[] = ['记录', '字数', '时长'];
const METRIC_COUNT: number = 0;
const METRIC_CHARS: number = 1;
const METRIC_DURATION: number = 2;
const BAR_MAX: number = 88;
const BAR_GAP: number = 2;

function pad2(value: number): string {
  return value < 10 ? '0' + value.toString() : value.toString();
}

/**
 * Overview v2: one primary action, then a composable grid of information cards.
 *
 *   voice card    status dot + timer, the microphone control, a phase hint, cancel
 *   setup banner  shown until an ASR key exists
 *   result card   the transcript that was just inserted, with copy
 *   card grid     今日 / 趋势 / 热力图 / 最近 / 配置 / 快捷入口, 1-3 columns by
 *                 breakpoint; which cards exist, and in what order, is the user's
 *                 choice (`Dashboard`, persisted in local preferences)
 *
 * The page owns no dictation state: `dictPhase` / `dictLevel` / `dictError` come
 * from `AppStorage` (published by `HarmonyApi`), and `backendRevision` triggers a
 * reload when another page changed settings or credentials.
 */
@Component
export struct OverviewPage {
  @StorageLink('bp') bp: string = 'md';
  @StorageLink('dictPhase') @Watch('onPhase') phase: string = 'idle';
  @StorageLink('dictLevel') level: number = 0;
  @StorageLink('dictError') dictError: string = '';
  @StorageLink('backendRevision') @Watch('onRevision') revision: number = 0;

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
  @State private totalCount: number = 0;
  @State private totalChars: number = 0;
  @State private weeks: WeekColumn[] = [];

  /** Trend card: the selection is the view state, the series is derived from activity. */
  @State private buckets: ActivityBucket[] = [];
  @State private periodTotal: number = 0;
  @State private periodMax: number = 0;
  @State private periodDays: number = 7;
  @State private periodMetric: number = METRIC_COUNT;

  /** Home composition, edited from the 自定义 sheet. */
  @State private visible: string[] = [];
  @State private customOpen: boolean = false;

  /** Elapsed seconds while the recorder / engine is busy, shown as mm:ss. */
  @State private elapsed: number = 0;
  private timerId: number = -1;
  private busySince: number = 0;

  aboutToAppear(): void {
    Api.takeSplashPlayback();
    this.loadCards();
    this.reload();
    if (this.phaseBusy()) {
      this.startTimer();
    }
  }

  aboutToDisappear(): void {
    this.stopTimer();
  }

  // ── state derivations ──────────────────────────────────────────────────────

  private phaseBusy(): boolean {
    return this.phase === 'starting' || this.phase === 'recording' ||
      this.phase === 'processing' || this.phase === 'cancelling';
  }

  private failureText(): string {
    if (this.error.length > 0) return this.error;
    return this.dictError;
  }

  private statusTitle(): string {
    if (this.phase === 'starting') return '正在准备麦克风';
    if (this.phase === 'recording') return '正在聆听';
    if (this.phase === 'processing') return '正在转写与润色';
    if (this.phase === 'cancelling') return '正在取消';
    if (this.failureText().length > 0) return '上次录音没有成功';
    if (!this.configured) return '配好密钥就能开始';
    return '点一下，开始说话';
  }

  private statusTone(): Resource {
    if (this.phase === 'recording') return Palette.danger();
    if (this.phaseBusy()) return Palette.accent();
    if (this.failureText().length > 0) return Palette.danger();
    return this.configured ? Palette.accent() : Palette.text3();
  }

  private phaseHint(): string {
    if (this.phase === 'starting') return '还没开始录音，现在仍可以取消';
    if (this.phase === 'recording') return '再点一下结束并上屏，说完直接停也可以';
    if (this.phase === 'processing') return '麦克风已释放，正在等引擎返回文字';
    if (this.phase === 'cancelling') return '正在丢弃这一段录音';
    if (this.failureText().length > 0) return '点「重新开始」再试一次，或先检查凭据';
    if (!this.configured) return '去「凭据与密钥」填一个转写密钥，录音才能变成文字';
    return '录音在本机完成，只有转写请求会上传音频';
  }

  private clockText(): string {
    const minutes: number = Math.floor(this.elapsed / 60);
    const seconds: number = this.elapsed % 60;
    return pad2(minutes) + ':' + pad2(seconds);
  }

  private greeting(): string {
    const hour: number = new Date().getHours();
    if (hour < 6) return '夜深了';
    if (hour < 12) return '早上好';
    if (hour < 18) return '下午好';
    return '晚上好';
  }

  private subtitleText(): string {
    const suffix: string = this.configured ? '说一句就上屏' : '还差一个转写密钥';
    return this.greeting() + ' · ' + suffix;
  }

  private microphoneLabel(): string {
    const name: string | undefined = this.prefs.microphoneDeviceName;
    if (name === undefined || name.length === 0) return '系统默认';
    return name;
  }

  private gridColumns(): number {
    if (this.bp === Layout.XL) return 3;
    if (this.bp === Layout.MD || this.bp === Layout.LG) return 2;
    return 1;
  }

  private isVisible(id: string): boolean {
    return this.visible.indexOf(id) >= 0;
  }

  // ── loading ────────────────────────────────────────────────────────────────

  private async reload(): Promise<void> {
    try {
      const activity: ActivityDay[] = await Api.activity();
      this.activity = activity;
      this.computeToday(activity);
      this.weeks = this.buildWeeks(activity);
      this.rebuildPeriod(activity);
      const settings: SettingsSnapshot = await Api.settings();
      this.prefs = settings.preferences;
      const history: HistoryEntry[] = await Api.history();
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

  /** Another page changed the backend (settings, credentials); pick it up. */
  private onRevision(): void {
    this.reload();
  }

  private async loadCards(): Promise<void> {
    try {
      const context = this.getUIContext().getHostContext() as common.UIAbilityContext;
      this.visible = await Dashboard.load(context);
    } catch (error) {
      this.visible = Dashboard.defaults();
    }
  }

  private async saveCards(): Promise<void> {
    try {
      const context = this.getUIContext().getHostContext() as common.UIAbilityContext;
      await Dashboard.save(context, this.visible);
    } catch (error) {
      console.warn('[OpenLess Overview] dashboard save skipped');
    }
  }

  private setCardVisible(id: string, enabled: boolean): void {
    const present: boolean = this.isVisible(id);
    if (present === enabled) return;
    if (!enabled && this.visible.length <= 1) {
      notify('至少保留一张卡片');
      return;
    }
    const next: string[] = [];
    if (enabled) {
      for (let index = 0; index < this.visible.length; index++) {
        next.push(this.visible[index]);
      }
      next.push(id);
    } else {
      for (let index = 0; index < this.visible.length; index++) {
        if (this.visible[index] !== id) next.push(this.visible[index]);
      }
    }
    this.visible = Dashboard.normalize(next);
    this.saveCards();
  }

  private restoreCards(): void {
    this.visible = Dashboard.defaults();
    this.saveCards();
    notify('已恢复默认卡片');
  }

  // ── metrics ────────────────────────────────────────────────────────────────

  private computeToday(activity: ActivityDay[]): void {
    const today: string = Fmt.todayIso();
    let count: number = 0;
    let chars: number = 0;
    let duration: number = 0;
    let sumCount: number = 0;
    let sumChars: number = 0;
    for (let index = 0; index < activity.length; index++) {
      const day: ActivityDay = activity[index];
      const dayCount: number = day.count;
      const dayChars: number = day.chars ?? 0;
      sumCount += dayCount;
      sumChars += dayChars;
      if (day.date === today) {
        count = dayCount;
        chars = dayChars;
        duration = day.durationMs ?? 0;
      }
    }
    this.todayCount = count;
    this.todayChars = chars;
    this.todayDuration = duration;
    this.totalCount = sumCount;
    this.totalChars = sumChars;
  }

  private metricValue(day: ActivityDay, metric: number): number {
    if (metric === METRIC_CHARS) return day.chars ?? 0;
    if (metric === METRIC_DURATION) return day.durationMs ?? 0;
    return day.count;
  }

  /** Continuous series of the last N days ending today; silent days are 0. */
  private rebuildPeriod(activity: ActivityDay[]): void {
    const byDate: Map<string, ActivityDay> = new Map<string, ActivityDay>();
    for (let index = 0; index < activity.length; index++) {
      byDate.set(activity[index].date, activity[index]);
    }
    const today: Date = new Date();
    const buckets: ActivityBucket[] = [];
    let total: number = 0;
    let max: number = 0;
    for (let offset = this.periodDays - 1; offset >= 0; offset--) {
      const cell: Date = new Date(today.getFullYear(), today.getMonth(), today.getDate() - offset);
      const iso: string = this.isoOf(cell);
      const day: ActivityDay | undefined = byDate.get(iso);
      const value: number = day === undefined ? 0 : this.metricValue(day, this.periodMetric);
      total += value;
      if (value > max) max = value;
      buckets.push({ date: iso, value: value });
    }
    this.buckets = buckets;
    this.periodTotal = total;
    this.periodMax = max;
  }

  private setPeriod(days: number): void {
    if (this.periodDays === days) return;
    this.periodDays = days;
    this.rebuildPeriod(this.activity);
  }

  private setMetric(metric: number): void {
    if (this.periodMetric === metric) return;
    this.periodMetric = metric;
    this.rebuildPeriod(this.activity);
  }

  private periodTotalValue(): string {
    if (this.periodMetric === METRIC_DURATION) {
      return this.periodTotal > 0 ? Fmt.duration(this.periodTotal) : '0:00';
    }
    return Fmt.count(this.periodTotal);
  }

  private periodUnit(): string {
    if (this.periodMetric === METRIC_COUNT) return '段';
    if (this.periodMetric === METRIC_CHARS) return '字';
    return '';
  }

  private periodSelect(): number {
    return this.periodDays === 7 ? 0 : 1;
  }

  /** Bar height: proportional to the period max, but visible for smaller values. */
  private barHeight(value: number): number {
    if (value <= 0) return 3;
    if (this.periodMax <= 0) return 3;
    const height: number = 3 + (value / this.periodMax) * (BAR_MAX - 6);
    return height < 4 ? 4 : height;
  }

  private isoOf(date: Date): string {
    return date.getFullYear().toString() + '-' + pad2(date.getMonth() + 1) + '-' + pad2(date.getDate());
  }

  /** Twelve weeks, Sunday-first like the HarmonyOS calendar. */
  private buildWeeks(activity: ActivityDay[]): WeekColumn[] {
    const counts: Map<string, number> = new Map<string, number>();
    for (let index = 0; index < activity.length; index++) {
      counts.set(activity[index].date, activity[index].count);
    }
    const today: Date = new Date();
    const columns: WeekColumn[] = [];
    for (let week = 11; week >= 0; week--) {
      const days: DayCell[] = [];
      for (let day = 0; day < 7; day++) {
        const cell: Date = new Date(today.getFullYear(), today.getMonth(),
          today.getDate() - (week * 7) - (6 - day));
        const iso: string = this.isoOf(cell);
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

  /** Visible cards, chunked into grid rows of `gridColumns()`. */
  private gridRows(): CardRow[] {
    const columns: number = this.gridColumns();
    const ids: string[] = Dashboard.normalize(this.visible);
    const rows: CardRow[] = [];
    for (let index = 0; index < ids.length; index += columns) {
      const group: string[] = [];
      for (let offset = 0; offset < columns && index + offset < ids.length; offset++) {
        group.push(ids[index + offset]);
      }
      rows.push({ key: 'row-' + index.toString(), ids: group });
    }
    return rows;
  }

  // ── busy timer ─────────────────────────────────────────────────────────────

  private onPhase(): void {
    if (this.phaseBusy()) {
      this.startTimer();
    } else {
      this.stopTimer();
    }
  }

  private startTimer(): void {
    if (this.timerId >= 0) return;
    this.busySince = Date.now();
    this.elapsed = 0;
    this.timerId = setInterval(() => {
      this.elapsed = Math.floor((Date.now() - this.busySince) / 1000);
    }, 500);
  }

  private stopTimer(): void {
    if (this.timerId >= 0) {
      clearInterval(this.timerId);
      this.timerId = -1;
    }
    this.elapsed = 0;
  }

  // ── dictation ──────────────────────────────────────────────────────────────

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

  private async copyResult(): Promise<void> {
    try {
      await Api.copyText(this.result);
      notify('已复制');
    } catch (error) {
      notify('复制失败');
    }
  }

  // ── view ───────────────────────────────────────────────────────────────────

  build() {
    PageScaffold({
      title: '概览',
      subtitle: this.subtitleText(),
      actionLabel: '自定义',
      actionSymbol: 'settings',
      onAction: () => {
        this.customOpen = true;
      }
    }) {
      this.voiceCard()
      if (!this.configured) {
        this.setupBanner()
      }
      this.resultCard()
      this.grid()
      this.sheetHost()
    }
  }

  /** Zero-size hosts: sheets are page-level, their content reads the state above. */
  @Builder
  sheetHost() {
    Column() {
      Row()
        .width(0)
        .height(0)
        .bindSheet($$this.customOpen, this.customSheet, {
          height: SheetSize.MEDIUM,
          dragBar: true,
          showClose: true,
          onDisappear: () => {
            this.customOpen = false;
          }
        })
    }
    .width('100%')
  }

  /** One tap target for the whole dictation loop; no second, competing button. */
  @Builder
  voiceCard() {
    Surface({ inset: Space.md }) {
      Column({ space: Space.sm }) {
        Row({ space: Space.xs }) {
          Column()
            .width(8)
            .height(8)
            .borderRadius(Radius.pill)
            .backgroundColor(this.statusTone())
          Text(this.statusTitle())
            .fontSize(Type.label)
            .lineHeight(Leading.label)
            .fontWeight(FontWeight.Medium)
            .fontColor(Palette.text1())
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
            .layoutWeight(1)
          if (this.phaseBusy()) {
            Text(this.clockText())
              .fontSize(Type.label)
              .lineHeight(Leading.label)
              .fontColor(Palette.text2())
          } else {
            Text('中英混合自动识别')
              .fontSize(Type.label)
              .lineHeight(Leading.label)
              .fontColor(Palette.text3())
              .maxLines(1)
          }
        }
        .width('100%')
        .constraintSize({ minHeight: Size.touch })
        .alignItems(VerticalAlign.Center)

        VoiceControl({
          phase: this.phase,
          level: this.level,
          usable: !this.busy,
          onToggle: () => {
            this.toggleDictation();
          }
        })

        if (this.phaseHint().length > 0) {
          Text(this.phaseHint())
            .fontSize(Type.label)
            .lineHeight(Leading.label)
            .fontColor(Palette.text3())
            .textAlign(TextAlign.Center)
            .width('100%')
        }

        if (this.phaseBusy()) {
          Row() {
            TextButton({
              label: '取消',
              tone: 'danger',
              onTap: () => {
                this.cancelDictation();
              }
            })
          }
          .width('100%')
          .justifyContent(FlexAlign.Center)
        }

        if (this.failureText().length > 0) {
          Banner({
            title: '这次没有成功',
            text: this.failureText(),
            tone: 'danger',
            action: '重新开始',
            actionSymbol: 'mic',
            onAction: () => {
              this.toggleDictation();
            }
          })
        }

        Flex({ wrap: FlexWrap.Wrap, justifyContent: FlexAlign.Center }) {
          Tag({ text: this.prefs.polishMode === 'raw' ? '原文模式' : '润色模式', tone: 'accent' })
          Tag({ text: Labels.provider(this.prefs.activeAsrProvider), tone: 'neutral' })
          Tag({ text: this.online ? '在线' : '离线', tone: this.online ? 'success' : 'warning' })
        }
        .width('100%')
      }
      .width('100%')
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

  @Builder
  resultCard() {
    if (this.result.length > 0) {
      Surface({ inset: Space.md }) {
        Column({ space: Space.xs }) {
          SectionTitle({
            title: '识别结果',
            action: '复制',
            onAction: () => {
              this.copyResult();
            }
          })
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

  // ── card grid ──────────────────────────────────────────────────────────────

  @Builder
  grid() {
    Column({ space: Space.md }) {
      ForEach(this.gridRows(), (row: CardRow) => {
        Row({ space: Space.md }) {
          ForEach(row.ids, (id: string) => {
            Column() {
              this.cardFor(id)
            }
            .layoutWeight(1)
            .alignItems(HorizontalAlign.Start)
          }, (id: string) => 'grid-' + id)
        }
        .width('100%')
        .alignItems(VerticalAlign.Top)
      }, (row: CardRow) => row.key)
    }
    .width('100%')
  }

  @Builder
  cardFor(id: string) {
    if (id === Dashboard.TODAY) {
      this.todayCard()
    } else if (id === Dashboard.PERIOD) {
      this.periodCard()
    } else if (id === Dashboard.HEATMAP) {
      this.heatmapCard()
    } else if (id === Dashboard.RECENT) {
      this.recentCard()
    } else if (id === Dashboard.CONFIG) {
      this.configCard()
    } else if (id === Dashboard.SHORTCUTS) {
      this.shortcutsCard()
    }
  }

  @Builder
  todayCard() {
    Surface({ inset: Space.md }) {
      Column({ space: Space.sm }) {
        SectionTitle({ title: '今日数据', hint: '统计只留在这一台设备' })
        Row({ space: Space.sm }) {
          MetricBlock({ value: Fmt.count(this.todayChars), label: '字数' })
          MetricBlock({ value: Fmt.duration(this.todayDuration), label: '时长' })
          MetricBlock({ value: this.todayCount.toString(), label: '段数' })
        }
        .width('100%')
        .alignItems(VerticalAlign.Bottom)

        Divider()
          .color(Palette.line())
          .strokeWidth(0.5)

        Row({ space: Space.sm }) {
          MetricBlock({ value: Fmt.count(this.totalCount), label: '累计记录', unit: '段' })
          MetricBlock({ value: Fmt.count(this.totalChars), label: '累计字数', unit: '字' })
        }
        .width('100%')
        .alignItems(VerticalAlign.Bottom)
      }
      .width('100%')
    }
  }

  @Builder
  periodCard() {
    Surface({ inset: Space.md }) {
      Column({ space: Space.sm }) {
        SectionTitle({ title: '最近趋势', hint: '把听写量按天摊开' })
        Segmented({
          items: PERIOD_LABELS,
          selected: this.periodSelect(),
          onSelect: (index: number) => {
            this.setPeriod(index === 0 ? 7 : 30);
          }
        })
        Segmented({
          items: METRIC_LABELS,
          selected: this.periodMetric,
          onSelect: (index: number) => {
            this.setMetric(index);
          }
        })

        Row({ space: BAR_GAP }) {
          ForEach(this.buckets, (bucket: ActivityBucket) => {
            Column() {
              Column()
                .width('100%')
                .height(this.barHeight(bucket.value))
                .backgroundColor(Palette.accent())
                .borderRadius(Radius.control)
                .opacity(0.85)
            }
            .layoutWeight(1)
            .height(BAR_MAX)
            .justifyContent(FlexAlign.End)
          }, (bucket: ActivityBucket) => 'bar-' + bucket.date)
        }
        .width('100%')
        .height(BAR_MAX)
        .alignItems(VerticalAlign.Bottom)

        Row({ space: Space.sm }) {
          MetricBlock({ value: this.periodTotalValue(), label: '总计', unit: this.periodUnit() })
        }
        .width('100%')
        .alignItems(VerticalAlign.Bottom)
      }
      .width('100%')
    }
  }

  @Builder
  heatmapCard() {
    Surface({ inset: Space.md }) {
      Column({ space: Space.sm }) {
        SectionTitle({ title: '活动热力图', hint: '近 12 周，一格一天' })
        Row() {
          ForEach(this.weeks, (week: WeekColumn) => {
            Column({ space: 3 }) {
              ForEach(week.days, (day: DayCell) => {
                Column()
                  .width(11)
                  .height(11)
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
            .lineHeight(Leading.caption)
            .fontColor(Palette.text3())
          ForEach(HEAT_LEVELS, (level: number) => {
            Column()
              .width(9)
              .height(9)
              .backgroundColor(Palette.heat(level))
              .borderRadius(2)
          }, (level: number) => 'legend-' + level.toString())
          Text('多')
            .fontSize(Type.caption)
            .lineHeight(Leading.caption)
            .fontColor(Palette.text3())
        }
        .width('100%')
        .justifyContent(FlexAlign.End)
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
            title: '最近记录',
            action: '全部历史',
            onAction: () => Nav.go(Nav.HISTORY)
          })
        }
        .width('100%')
        .padding({ left: Space.md, right: Space.md, top: Space.sm, bottom: Space.xxs })

        if (this.recent.length === 0) {
          EmptyHint({ symbol: 'history', title: '还没有听写记录', hint: '说第一句话就会出现在这里' })
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
          }, (entry: HistoryEntry) => 'recent-' + entry.id)
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
          SectionTitle({ title: '当前配置', hint: '点一行去设置里改' })
        }
        .width('100%')
        .padding({ left: Space.md, right: Space.md, top: Space.sm, bottom: Space.xxs })

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
  shortcutsCard() {
    Surface({ inset: Space.md }) {
      Column({ space: Space.sm }) {
        SectionTitle({ title: '快捷入口', hint: '常用工具一步可达' })
        Flex({ wrap: FlexWrap.Wrap }) {
          this.shortcutButton('翻译', 'translation', Nav.TRANSLATION)
          this.shortcutButton('词典', 'vocab', Nav.VOCAB)
          this.shortcutButton('纠错规则', 'corrections', Nav.CORRECTIONS)
          this.shortcutButton('选取助手', 'person', Nav.SELECTION)
        }
        .width('100%')
      }
      .width('100%')
    }
  }

  @Builder
  shortcutButton(label: string, symbol: string, route: string) {
    Row() {
      TonalButton({
        label: label,
        symbol: symbol,
        onTap: () => Nav.go(route)
      })
    }
    .margin({ right: Space.xs, bottom: Space.xs })
  }

  // ── 自定义主页 sheet ───────────────────────────────────────────────────────

  @Builder
  customSheet() {
    Column({ space: Space.sm }) {
      SheetTitle({
        title: '自定义主页',
        hint: '关掉的卡片不会显示在主页上，改动会立刻生效'
      })
      Scroll() {
        Column() {
          Column() {
            Group() {
              ForEach(Dashboard.CARDS, (card: DashboardCard, index: number) => {
                SettingSwitch({
                  label: card.title,
                  hint: card.hint,
                  checked: this.isVisible(card.id),
                  showDivider: index < Dashboard.CARDS.length - 1,
                  onToggle: (value: boolean) => {
                    this.setCardVisible(card.id, value);
                  }
                })
              }, (card: DashboardCard) => 'card-' + card.id)
            }
          }
          .width('100%')
        }
        .width('100%')
      }
      .layoutWeight(1)
      .width('100%')
      .align(Alignment.Top)
      .scrollBar(BarState.Off)
      .edgeEffect(EdgeEffect.Spring)

      Row() {
        TextButton({
          label: '恢复默认',
          symbol: 'refresh',
          onTap: () => {
            this.restoreCards();
          }
        })
      }
      .width('100%')
      .justifyContent(FlexAlign.Center)
      .padding({ top: Space.xs, bottom: Space.sm })
    }
    .width('100%')
    .height('100%')
    .alignItems(HorizontalAlign.Start)
    .padding({ left: Space.md, right: Space.md })
  }
}
