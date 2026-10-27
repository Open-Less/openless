import { Breakpoints } from '../common/Breakpoint';
import { Fmt } from '../common/Format';
import { Labels } from '../common/Labels';
import { Store } from '../common/Store';
import { C, R, S, T, cardShadow } from '../common/Tokens';
import { Card, EmptyState, GhostButton, MetricTile, Pill, PrimaryButton, SectionHeader, StatusRow } from '../components/Common';
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
 * Overview: the one screen a user lands on.
 *
 * It owns the whole dictation loop (start, level feedback, stop, result) and the
 * at-a-glance state of the backend. Wide layouts put the dictation hero and the
 * activity panel side by side; the compact layout stacks them.
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
    this.reload();
  }

  /** Re-reads the panels a dictation or a settings change can invalidate. */
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

  /** Twelve weeks of activity, Sunday-first like the HarmonyOS calendar. */
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
        const count: number = counts.get(iso) ?? 0;
        days.push({ date: iso, count: count });
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

  private microphoneLabel(): string {
    const name: string | undefined = this.prefs.microphoneDeviceName;
    if (name === undefined || name.length === 0) return '系统默认';
    return name;
  }

  private phaseTitle(): string {
    if (this.phase === 'starting') return '正在准备麦克风…';
    if (this.phase === 'recording') return '正在聆听';
    if (this.phase === 'processing') return '正在转写与润色…';
    if (this.phase === 'cancelling') return '正在取消…';
    if (this.dictError.length > 0) return '上次录音失败';
    return '点一下，开始说话';
  }

  private phaseHint(): string {
    if (this.dictError.length > 0) return this.dictError;
    if (this.phase === 'recording') return '再次点击结束并上屏；说完直接停也行';
    if (this.phase === 'idle') return '录音在设备本地完成，只有转写请求会发送音频';
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
    Column() {
      Row({ space: S.md }) {
        Column({ space: S.xxs }) {
          Text(this.greeting())
            .fontSize(T.footnote)
            .fontColor(C.textTertiary())
          Text('概览')
            .fontSize(T.display)
            .fontWeight(FontWeight.Medium)
            .fontColor(C.textPrimary())
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)

        GhostButton({
          label: '刷新',
          symbol: 'refresh',
          onTap: () => {
            this.reload();
          }
        })
      }
      .width('100%')
      .padding({ top: S.lg, bottom: S.md })

      Scroll() {
        if (Breakpoints.isWide(this.bp)) {
          Row({ space: S.md }) {
            Column({ space: S.md }) {
              this.dictationCard()
              this.resultCard()
            }
            .layoutWeight(5)
            .alignItems(HorizontalAlign.Start)

            Column({ space: S.md }) {
              this.metricsCard()
              this.activityCard()
              this.statusCard()
              this.recentCard()
            }
            .layoutWeight(4)
            .alignItems(HorizontalAlign.Start)
          }
          .width('100%')
          .alignItems(VerticalAlign.Top)
        } else {
          Column({ space: S.md }) {
            this.onboardingCard()
            this.dictationCard()
            this.resultCard()
            this.metricsCard()
            this.activityCard()
            this.statusCard()
            this.recentCard()
          }
          .width('100%')
        }
      }
      .scrollBar(BarState.Off)
      .layoutWeight(1)
      .align(Alignment.Top)
    }
    .width('100%')
    .height('100%')
    .padding({ left: S.gutter(this.bp), right: S.gutter(this.bp) })
  }

  /** First run: the app cannot transcribe until a provider key exists. */
  @Builder
  onboardingCard() {
    if (!this.configured) {
      Card() {
        Column({ space: S.sm }) {
          Row({ space: S.sm }) {
            SymbolGlyph(symbolOf('sparkles'))
              .fontSize(20)
              .fontColor([C.accent()])
            Text('先配一个转写密钥')
              .fontSize(T.headline)
              .fontWeight(FontWeight.Medium)
              .fontColor(C.textPrimary())
              .layoutWeight(1)
          }
          .width('100%')
          Text('鸿蒙端把转写交给云端引擎，密钥存在系统密钥库。没有密钥时录音会失败，历史里只会留下错误码。')
            .fontSize(T.footnote)
            .fontColor(C.textSecondary())
            .lineHeight(21)
            .width('100%')
          Row({ space: S.sm }) {
            GhostButton({
              label: '去配凭据',
              symbol: 'settings',
              onTap: () => Nav.go(Nav.CREDENTIALS)
            })
            GhostButton({
              label: '查看引擎',
              onTap: () => Nav.go(Nav.SETTINGS)
            })
          }
          .width('100%')
        }
        .width('100%')
      }
    }
  }

  @Builder
  dictationCard() {
    Card() {
      Column({ space: S.md }) {
        Column({ space: S.sm }) {
          Stack() {
            Circle()
              .width(180)
              .height(180)
              .fill(C.accentSoft())
              .opacity(0.25)
            Circle()
              .width(148)
              .height(148)
              .fill(C.accentSoft())
              .opacity(this.phase === 'recording' ? 0.55 + this.level * 0.45 : 0.5)
              .scale({ x: 1 + this.level * 0.08, y: 1 + this.level * 0.08 })
              .animation({ duration: 120, curve: Curve.EaseOut })

            Circle()
              .width(108)
              .height(108)
              .fill(this.phase === 'recording' ? C.accent() : C.accentSoft())
              .shadow({
                radius: this.phase === 'recording' ? 32 : 16,
                color: C.accentSoft(),
                offsetX: 0,
                offsetY: 6
              })

            SymbolGlyph(symbolOf(this.phase === 'recording' ? 'stop' : 'mic'))
              .fontSize(42)
              .fontColor([this.phase === 'recording' ? C.textOnAccent() : C.accent()])
          }
          .width(160)
          .height(160)
          .onClick(() => {
            this.toggleDictation();
          })

          Text(this.phaseTitle())
            .fontSize(T.headline)
            .fontWeight(FontWeight.Medium)
            .fontColor(C.textPrimary())
          if (this.phaseHint().length > 0) {
            Text(this.phaseHint())
              .fontSize(T.footnote)
              .fontColor(this.dictError.length > 0 ? C.danger() : C.textTertiary())
              .textAlign(TextAlign.Center)
          }
          if (this.error.length > 0) {
            Text(this.error)
              .fontSize(T.footnote)
              .fontColor(C.danger())
              .textAlign(TextAlign.Center)
          }
        }
        .width('100%')
        .alignItems(HorizontalAlign.Center)
        .padding({ top: S.md, bottom: S.xs })

        Row({ space: S.sm }) {
          if (Breakpoints.isCompact(this.bp)) {
            PrimaryButton({
              label: this.phase === 'recording' ? '结束并上屏' : '开始说话',
              usable: !this.busy && (this.phase === 'idle' || this.phase === 'recording' || this.dictError.length > 0),
              onTap: () => {
                this.toggleDictation();
              }
            })
            if (this.phase === 'recording') {
              GhostButton({
                label: '取消',
                onTap: () => {
                  this.cancelDictation();
                }
              })
            }
          } else {
            PrimaryButton({
              label: this.phase === 'recording' ? '结束并上屏' : '开始说话',
              usable: !this.busy && (this.phase === 'idle' || this.phase === 'recording' || this.dictError.length > 0),
              onTap: () => {
                this.toggleDictation();
              }
            })
            if (this.phase === 'recording') {
              GhostButton({
                label: '取消录音',
                onTap: () => {
                  this.cancelDictation();
                }
              })
            }
          }
        }
        .width('100%')
        .justifyContent(FlexAlign.Center)

        Row({ space: S.xs }) {
          Pill({ text: this.prefs.polishMode === 'raw' ? '原文模式' : '润色模式', tone: 'accent' })
          Pill({ text: Labels.provider(this.prefs.activeAsrProvider), tone: 'neutral' })
          Pill({ text: this.online ? '在线' : '离线', tone: this.online ? 'success' : 'warning' })
        }
        .width('100%')
        .justifyContent(FlexAlign.Center)
        .padding({ bottom: S.xs })
      }
      .width('100%')
    }
  }

  @Builder
  resultCard() {
    if (this.result.length > 0) {
      Card() {
        Column({ space: S.sm }) {
          Row({ space: S.sm }) {
            Text('刚刚上屏')
              .fontSize(T.callout)
              .fontWeight(FontWeight.Medium)
              .fontColor(C.textPrimary())
              .layoutWeight(1)
            GhostButton({
              label: '复制',
              symbol: 'copy',
              onTap: () => {
                Api.copyText(this.result);
              }
            })
          }
          .width('100%')

          Text(this.result)
            .fontSize(T.body)
            .fontColor(C.textPrimary())
            .lineHeight(24)
            .width('100%')
            .copyOption(CopyOptions.LocalDevice)
        }
        .width('100%')
      }
    }
  }

  @Builder
  metricsCard() {
    Card() {
      Column({ space: S.md }) {
        SectionHeader({ title: '今天', hint: '设备本地统计，不上传' })
        Row() {
          MetricTile({ label: '段数', value: this.todayCount.toString(), hero: true })
          MetricTile({ label: '字数', value: Fmt.count(this.todayChars), hero: true })
          MetricTile({ label: '时长', value: Fmt.duration(this.todayDuration), hero: true })
        }
        .width('100%')
        .alignItems(VerticalAlign.Bottom)
      }
      .width('100%')
    }
  }

  @Builder
  activityCard() {
    Card() {
      Column({ space: S.md }) {
        SectionHeader({ title: '近 12 周', hint: '每格代表一天，颜色越深说得越多' })
        Row({ space: S.xs }) {
          ForEach(this.weeks, (week: WeekColumn) => {
            Column({ space: S.xs }) {
              ForEach(week.days, (day: DayCell) => {
                Column()
                  .width(14)
                  .height(14)
                  .backgroundColor(C.heat(this.heatLevel(day.count)))
                  .borderRadius(4)
              }, (day: DayCell) => day.date)
            }
          }, (week: WeekColumn) => week.key)
        }
        .width('100%')
        .justifyContent(FlexAlign.SpaceBetween)

        Row({ space: S.xs }) {
          Text('少')
            .fontSize(T.caption)
            .fontColor(C.textTertiary())
          ForEach(HEAT_LEVELS, (level: number) => {
            Column()
              .width(10)
              .height(10)
              .backgroundColor(C.heat(level))
              .borderRadius(3)
          }, (level: number) => 'legend-' + level.toString())
          Text('多')
            .fontSize(T.caption)
            .fontColor(C.textTertiary())
        }
      }
      .width('100%')
    }
  }

  @Builder
  statusCard() {
    Card() {
      Column() {
        SectionHeader({ title: '当前配置' })
        StatusRow({
          label: '转写引擎',
          value: Labels.provider(this.prefs.activeAsrProvider),
          onTap: () => Nav.go(Nav.SETTINGS)
        })
        StatusRow({
          label: '润色模型',
          value: Labels.provider(this.prefs.activeLlmProvider),
          onTap: () => Nav.go(Nav.SETTINGS)
        })
        StatusRow({
          label: '麦克风',
          value: this.microphoneLabel(),
          onTap: () => Nav.go(Nav.SETTINGS)
        })
        StatusRow({
          label: '网络',
          value: this.online ? '已连接' : '未连接',
          onTap: () => Nav.go(Nav.SETTINGS)
        })
        StatusRow({
          label: '离线听写',
          value: this.capabilities.supportsLocalAsr === true ? '可用' : '需联网转写',
          showDivider: false,
          onTap: () => Nav.go(Nav.LOCALASR)
        })
      }
      .width('100%')
    }
  }

  @Builder
  recentCard() {
    Card() {
      Column({ space: S.sm }) {
        Row({ space: S.sm }) {
          Text('最近')
            .fontSize(T.headline)
            .fontWeight(FontWeight.Medium)
            .fontColor(C.textPrimary())
            .layoutWeight(1)
          GhostButton({
            label: '全部历史',
            onTap: () => Nav.go(Nav.HISTORY)
          })
        }
        .width('100%')

        if (this.recent.length === 0) {
          EmptyState({ symbol: 'history', title: '还没有听写记录', hint: '点上面的麦克风说第一句话' })
        } else {
          ForEach(this.recent, (entry: HistoryEntry) => {
            Row({ space: S.sm }) {
              Column({ space: S.xxs }) {
                Text(Fmt.preview(entry.finalText, 40))
                  .fontSize(T.callout)
                  .fontColor(C.textPrimary())
                  .maxLines(2)
                  .textOverflow({ overflow: TextOverflow.Ellipsis })
                Text(Fmt.relative(entry.createdAt))
                  .fontSize(T.caption)
                  .fontColor(C.textTertiary())
              }
              .alignItems(HorizontalAlign.Start)
              .layoutWeight(1)
            }
            .width('100%')
            .padding({ top: S.sm, bottom: S.sm })
            .onClick(() => {
              Nav.go(Nav.HISTORY);
              Store.put<string>('detailId', entry.id);
            })
          }, (entry: HistoryEntry) => entry.id)
        }
      }
      .width('100%')
    }
  }
}
