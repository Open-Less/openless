import { LengthMetrics } from '@kit.ArkUI';
import { common } from '@kit.AbilityKit';
import { Dashboard, DashboardCard } from '../common/Dashboard';
import { Fmt } from '../common/Format';
import { Labels } from '../common/Labels';
import { Store } from '../common/Store';
import { Layout, Leading, Palette, Radius, Size, Space, Type } from '../common/Theme';
import {
  EmptyHint,
  Group,
  RowItem,
  SectionTitle,
  SettingSwitch,
  SheetTitle,
  Surface,
  TextButton,
  TonalButton,
  notify
} from '../components/Kit';
import { symbolOf } from '../components/Icons';
import { Nav } from '../components/Nav';
import { AdaptiveColumns } from '../components/AdaptiveLayout';
import { BrandBar } from '../components/overview/BrandBar';
import { VoiceSessionCard } from '../components/overview/VoiceSessionCard';
import { SpeechErrorCard } from '../components/overview/SpeechErrorCard';
import { TranscriptCard } from '../components/overview/TranscriptCard';
import { TodayMetricsCard } from '../components/overview/TodayMetricsCard';
import { ActivityBucket, ActivityTrendCard } from '../components/overview/ActivityTrendCard';
import { Api } from '../data/Api';
import { ActivityDay, HistoryEntry, NativeReply, PlatformCapabilities, SettingsSnapshot, UserPreferences } from '../data/Types';

/** One day of the 12-week heat map. */
interface DayCell {
  date: string;
  count: number;
}

interface WeekColumn {
  key: string;
  days: DayCell[];
}

const HEAT_LEVELS: number[] = [0, 1, 2, 3, 4];
const METRIC_COUNT: number = 0;
const METRIC_CHARS: number = 1;
const METRIC_DURATION: number = 2;
/** iOS resets 「已复制」 after two seconds. */
const COPY_RESET_MS: number = 2000;
const RECENT_LIMIT: number = 3;

function pad2(value: number): string {
  return value < 10 ? '0' + value.toString() : value.toString();
}

/**
 * Overview page, a line-by-line port of iOS `HomeView.swift` (body 116-183) and
 * `OverviewMetricsView.swift`:
 *
 *   brand row      OpenLess mark 34 + wordmark + keyboard setup button
 *   voice card     status dot / state / timer-language, mic control, hint, cancel
 *   error card     only after a failed request (danger), optional system settings
 *   result card    transcript with copy / 已复制, selectable text
 *   概览            title3 heading + refresh, 2×2 metrics card, trend card
 *   footnote       keyboard-voice boundary note
 *   dashboard      Harmony-only user-selected cards, kept after the iOS content
 *
 * The page owns every API call and all recording/timer state; the `overview/`
 * components receive props and plain callbacks only. `dictPhase` / `dictLevel` /
 * `dictError` still come from `AppStorage` (published by `HarmonyApi`), and
 * `backendRevision` still triggers a reload when another page writes settings.
 */
@Component
export struct OverviewPage {
  @StorageLink('bp') bp: string = 'md';
  @StorageLink('phoneLayout') phoneLayout: boolean = true;
  @StorageLink('dictPhase') @Watch('onPhase') phase: string = 'idle';
  @StorageLink('dictLevel') level: number = 0;
  @StorageLink('reduceMotion') reduceMotion: boolean = false;
  @StorageLink('dictError') dictError: string = '';
  @StorageLink('backendRevision') @Watch('onRevision') revision: number = 0;

  @State private busy: boolean = false;
  @State private error: string = '';
  @State private result: string = '';
  @State private copied: boolean = false;
  @State private micDenied: boolean = false;
  @State private loading: boolean = false;
  @State private metricsLoaded: boolean = false;
  @State private dataError: string = '';
  @State private activity: ActivityDay[] = [];
  @State private recent: HistoryEntry[] = [];
  @State private totalRecords: number = 0;
  @State private prefs: UserPreferences = {};
  @State private online: boolean = true;
  @State private capabilities: PlatformCapabilities = { platform: 'harmony' };
  @State private configured: boolean = true;
  @State private todayCount: number = 0;
  @State private todayChars: number = 0;
  @State private todayDuration: number = 0;
  @State private weeks: WeekColumn[] = [];

  /** Trend card: the selection is the view state, the series is derived from activity. */
  @State private buckets: ActivityBucket[] = [];
  @State private periodTotal: number = 0;
  @State private periodMax: number = 0;
  @State private periodDays: number = 7;
  @State private periodMetric: number = METRIC_COUNT;

  /** Harmony-only home composition, edited from the 自定义 sheet. */
  @State private visible: string[] = [];
  @State private customOpen: boolean = false;

  /** Elapsed seconds while the recorder / engine is busy, shown as mm:ss. */
  @State private elapsed: number = 0;
  private timerId: number = -1;
  private copyTimer: number = -1;
  private busySince: number = 0;
  private reloadPending: boolean = false;

  aboutToAppear(): void {
    Api.takeSplashPlayback();
    this.loadCards();
    this.reload();
    this.loadMicPermission();
    if (this.phaseBusy()) {
      this.startTimer();
    }
  }

  aboutToDisappear(): void {
    // Teardown must not mutate @State (ArkUI custom-component lifecycle contract).
    if (this.timerId >= 0) {
      clearInterval(this.timerId);
      this.timerId = -1;
    }
    if (this.copyTimer >= 0) {
      clearTimeout(this.copyTimer);
      this.copyTimer = -1;
    }
  }

  // ── state derivations (iOS HomeView stateTitle / hint / statusColor) ───────

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
    return this.configured ? '可以开始了' : '首次使用，先启用语音';
  }

  private statusTone(): Resource {
    if (this.phase === 'recording') return Palette.danger();
    if (this.phaseBusy()) return Palette.accent();
    return this.configured ? Palette.accent() : Palette.text3();
  }

  private phaseHint(): string {
    if (this.phase === 'starting') return '还没开始录音，现在仍可以取消。';
    if (this.phase === 'recording') return '轻点结束。';
    if (this.phase === 'processing') return '麦克风已释放，请稍候。';
    if (this.phase === 'cancelling') return '正在丢弃这一段录音。';
    if (!this.configured) return '去「大模型配置」填一个语音渠道密钥，录音才能变成文字。';
    return '轻点开始，再点结束。';
  }

  private clockText(): string {
    const minutes: number = Math.floor(this.elapsed / 60);
    const seconds: number = this.elapsed % 60;
    return pad2(minutes) + ':' + pad2(seconds);
  }

  private microphoneLabel(): string {
    const name: string | undefined = this.prefs.microphoneDeviceName;
    if (name === undefined || name.length === 0) return '系统默认';
    return name;
  }

  private wide(): boolean {
    return !this.phoneLayout && (this.bp === Layout.LG || this.bp === Layout.XL);
  }

  /** Cards the iOS design already owns; the Dashboard must not duplicate them. */
  private fixedCard(id: string): boolean {
    return id === Dashboard.TODAY || id === Dashboard.PERIOD;
  }

  /** Preserve user ordering and drop the two cards now rendered by the iOS sections. */
  private dashboardIds(): string[] {
    const ids: string[] = Dashboard.normalize(this.visible);
    const selected: string[] = [];
    for (let index = 0; index < ids.length; index++) {
      if (!this.fixedCard(ids[index])) selected.push(ids[index]);
    }
    return selected;
  }

  private customCards(): DashboardCard[] {
    const cards: DashboardCard[] = [];
    for (let index = 0; index < Dashboard.CARDS.length; index++) {
      if (!this.fixedCard(Dashboard.CARDS[index].id)) cards.push(Dashboard.CARDS[index]);
    }
    return cards;
  }

  private isVisible(id: string): boolean {
    return this.visible.indexOf(id) >= 0;
  }

  // ── loading ────────────────────────────────────────────────────────────────

  private async reload(): Promise<void> {
    if (this.loading) {
      this.reloadPending = true;
      return;
    }
    this.loading = true;
    try {
      const activity: ActivityDay[] = await Api.activity();
      this.activity = activity;
      this.computeToday(activity);
      this.weeks = this.buildWeeks(activity);
      this.rebuildPeriod(activity);
      this.metricsLoaded = true;
      const settings: SettingsSnapshot = await Api.settings();
      this.prefs = settings.preferences;
      const history: HistoryEntry[] = await Api.history();
      this.recent = history.slice(0, RECENT_LIMIT);
      this.totalRecords = history.length;
      const network = await Api.network();
      this.online = network.online;
      this.capabilities = await Api.platformCapabilities();
      try {
        const credentials = await Api.credentialStatus();
        this.configured = credentials.pipelineMode === 'multimodal'
          ? credentials.omniConfigured === true
          : credentials.asrConfigured === true;
      } catch (error) {
        this.configured = false;
      }
      this.dataError = '';
    } catch (error) {
      this.dataError = (error as Error).message;
    } finally {
      this.loading = false;
      // A finished recording/settings write must not be lost behind an older refresh.
      if (this.reloadPending) {
        this.reloadPending = false;
        this.reload();
      }
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

  /** Real permission check, used only to decide the 「前往系统设置」 affordance. */
  private async loadMicPermission(): Promise<void> {
    try {
      const raw: string = await Api.raw('check_microphone_permission', '{}');
      const reply: NativeReply = JSON.parse(raw) as NativeReply;
      if (reply.ok && reply.value !== undefined && reply.value !== null) {
        const status: string = JSON.stringify(reply.value).split('"').join('');
        this.micDenied = status !== 'granted';
      }
    } catch (error) {
      this.micDenied = false;
    }
  }

  private async openMicSettings(): Promise<void> {
    try {
      await Api.openSystemMicrophoneSettings();
    } catch (error) {
      notify('无法打开系统设置');
    }
  }

  // ── metrics ────────────────────────────────────────────────────────────────

  private computeToday(activity: ActivityDay[]): void {
    const today: string = Fmt.todayIso();
    let count: number = 0;
    let chars: number = 0;
    let duration: number = 0;
    for (let index = 0; index < activity.length; index++) {
      const day: ActivityDay = activity[index];
      if (day.date === today) {
        count = day.count;
        chars = day.chars ?? 0;
        duration = day.durationMs ?? 0;
      }
    }
    this.todayCount = count;
    this.todayChars = chars;
    this.todayDuration = duration;
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
        this.copied = false;
        await Api.startDictation();
      } else {
        const voice = await Api.stopDictation();
        this.result = voice.polishedText.length > 0 ? voice.polishedText : voice.rawText;
        await this.reload();
      }
    } catch (error) {
      this.error = (error as Error).message;
      await this.loadMicPermission();
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
      this.copied = true;
      if (this.copyTimer >= 0) clearTimeout(this.copyTimer);
      this.copyTimer = setTimeout(() => {
        this.copied = false;
        this.copyTimer = -1;
      }, COPY_RESET_MS);
      notify('已复制');
    } catch (error) {
      notify('复制失败');
    }
  }

  // ── view: iOS HomeView body, then the Harmony-only dashboard ───────────────

  build() {
    Column() {
      Scroll() {
        Column() {
          AdaptiveColumns({
            bp: this.bp,
            singleColumn: this.phoneLayout,
            secondaryWidth: this.bp === Layout.XL ? 380 : 340,
            primary: () => {
              this.primaryContent();
            },
            secondary: () => {
              this.secondaryContent();
            }
          })
        }
        .width('100%')
        .padding({ top: Space.sm, bottom: Layout.dockClearance() })
      }
      .layoutWeight(1)
      .width('100%')
      .align(Alignment.Top)
      .scrollBar(BarState.Off)
      .edgeEffect(EdgeEffect.Spring)

      this.sheetHost()
    }
    .width('100%')
    .height('100%')
    .padding({ left: Layout.gutter(this.bp), right: Layout.gutter(this.bp) })
  }

  @Builder
  primaryContent() {
    Column({ space: Space.lg }) {
      BrandBar({
        onKeyboard: () => {
          Nav.go(Nav.SETTINGS);
        }
      })
      this.voiceCard()
      if (this.failureText().length > 0) {
        this.errorCard()
      }
      this.resultCard()
      this.overviewSection()
      this.keyboardFootnote()
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
  }

  @Builder
  secondaryContent() {
    Column({ space: Space.lg }) {
      this.dashboardHeader()
      ForEach(this.dashboardIds(), (id: string) => {
        this.cardFor(id)
      }, (id: string) => 'dash-' + id)
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
  }

  @Builder
  voiceCard() {
    VoiceSessionCard({
      phase: this.phase,
      level: this.level,
      busy: this.busy,
      active: this.phaseBusy(),
      title: this.statusTitle(),
      tone: this.statusTone(),
      clock: this.clockText(),
      hint: this.phaseHint(),
      configured: this.configured,
      onToggle: () => {
        this.toggleDictation();
      },
      onCancel: () => {
        this.cancelDictation();
      },
      onSetup: () => {
        Nav.go(Nav.CREDENTIALS);
      }
    })
  }

  @Builder
  errorCard() {
    SpeechErrorCard({
      message: this.failureText(),
      denied: this.micDenied,
      onSettings: () => {
        this.openMicSettings();
      }
    })
  }

  @Builder
  resultCard() {
    if (this.result.length > 0) {
      TranscriptCard({
        text: this.result,
        copied: this.copied,
        active: this.phaseBusy(),
        onCopy: () => {
          this.copyResult();
        }
      })
    }
  }

  /** iOS OverviewMetricsView: heading + refresh, 2×2 card, trend card. */
  @Builder
  overviewSection() {
    Column({ space: Space.sm }) {
      Row() {
        Text('概览')
          .fontSize(Type.section)
          .lineHeight(Leading.section)
          .fontWeight(FontWeight.Medium)
          .fontColor(Palette.text1())
        Blank()
        Button({ type: ButtonType.Circle }) {
          SymbolGlyph(symbolOf('refresh'))
            .fontSize(Type.navTitle)
            .fontColor([Palette.text2()])
        }
        .width(Size.touch)
        .height(Size.touch)
        .backgroundColor(Color.Transparent)
        .enabled(!this.loading)
        .opacity(this.loading ? 0.4 : 1)
        .hoverEffect(HoverEffect.Highlight)
        .accessibilityText('刷新概览')
        .onClick(() => {
          this.reload();
        })
      }
      .width('100%')
      .alignItems(VerticalAlign.Center)

      if (this.dataError.length > 0) {
        Row({ space: Space.xxs }) {
          SymbolGlyph(symbolOf('close'))
            .fontSize(Type.caption)
            .fontColor([Palette.text2()])
          Text('统计暂时无法载入。')
            .fontSize(Type.caption)
            .lineHeight(Leading.caption)
            .fontColor(Palette.text2())
        }
        .width('100%')
      }

      TodayMetricsCard({
        loaded: this.metricsLoaded,
        todayChars: this.todayChars,
        todayDuration: this.todayDuration,
        todayCount: this.todayCount,
        totalRecords: this.totalRecords
      })

      ActivityTrendCard({
        reduceMotion: this.reduceMotion,
        buckets: this.buckets,
        periodDays: this.periodDays,
        periodMetric: this.periodMetric,
        periodMax: this.periodMax,
        total: this.periodTotalValue(),
        loaded: this.metricsLoaded,
        empty: this.metricsLoaded && this.periodTotal === 0,
        onPeriod: (days: number) => {
          this.setPeriod(days);
        },
        onMetric: (metric: number) => {
          this.setMetric(metric);
        }
      })
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
  }

  /** iOS HomeView.swift:167-172, the closing footnote (plain text, no control). */
  @Builder
  keyboardFootnote() {
    Text('启用键盘语音后，可在其他应用中录音')
      .fontSize(Type.caption)
      .lineHeight(Leading.caption)
      .fontColor(Palette.text2())
      .maxLines(1)
      .textOverflow({ overflow: TextOverflow.Ellipsis })
      .width('100%')
  }

  /** Harmony-only: the dashboard stays reachable after the iOS content. */
  @Builder
  dashboardHeader() {
    Row({ space: Space.sm }) {
      Column({ space: Space.xxs }) {
        Text('自定义卡片')
          .fontSize(Type.label)
          .lineHeight(Leading.label)
          .fontWeight(FontWeight.Medium)
          .fontColor(Palette.text1())
        Text('按需显示，改动立刻生效')
          .fontSize(Type.caption)
          .lineHeight(Leading.caption)
          .fontColor(Palette.text2())
      }
      .alignItems(HorizontalAlign.Start)
      .layoutWeight(1)

      TextButton({
        label: '自定义',
        onTap: () => {
          this.customOpen = true;
        }
      })
    }
    .width('100%')
    .alignItems(VerticalAlign.Center)
  }

  /** Zero-size host: the sheet is page-level, its content reads the state above. */
  @Builder
  sheetHost() {
    Column() {
      Row()
        .width(0)
        .height(0)
        .bindSheet($$this.customOpen, this.customSheet, {
          // Wide windows use a centred dialog; bottom sheets above 600vp are
          // re-typed by the system and ignore the height below.
          // https://developer.huawei.com/consumer/cn/doc/harmonyos-references/ts-universal-attributes-sheet-transition
          preferType: this.wide() ? SheetType.CENTER : SheetType.BOTTOM,
          width: this.wide() ? 520 : '100%',
          height: this.wide() ? SheetSize.LARGE : SheetSize.MEDIUM,
          dragBar: true,
          radius: LengthMetrics.vp(Radius.sheet),
          showClose: true,
          onDisappear: () => {
            this.customOpen = false;
          }
        })
    }
    .width('100%')
  }

  // ── Harmony-only user-selected information cards ───────────────────────────

  @Builder
  cardFor(id: string) {
    if (id === Dashboard.HEATMAP) {
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
        hint: '关掉的卡片不会显示在主页上，改动会立刻生效；今日数据与使用趋势固定显示'
      })
      Scroll() {
        Column() {
          Column() {
            Group() {
              ForEach(this.customCards(), (card: DashboardCard, index: number) => {
                SettingSwitch({
                  label: card.title,
                  hint: card.hint,
                  checked: this.isVisible(card.id),
                  showDivider: index < this.customCards().length - 1,
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
