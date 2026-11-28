import { Leading, Motion, Palette, Space, Type } from '../../common/Theme';
import { symbolOf } from '../Icons';
import { Segmented, Surface } from '../Kit';

/**
 * iOS OverviewMetricsView.swift:32-61, the trend product card:
 *
 *   Picker(统计周期)            segmented, 近 7 天 / 近 30 天
 *   Text(total)                 title2 semibold, monospaced digits
 *   Picker(统计指标)            记录 / 字数 / 时长
 *   Chart                       112pt tall, accent bars, 3pt corner radius
 *   Text("最近的使用趋势…")       caption, secondary
 *
 * The SwiftUI chart is replaced by plain bars (no Charts port on ArkUI); one bar
 * per day of the selected window, first/last date labelled like the axis marks.
 * Selections are callbacks: the page owns the series, this card only draws it.
 */

export interface ActivityBucket {
  date: string;
  value: number;
}

const PERIOD_LABELS: string[] = ['近 7 天', '近 30 天'];
const METRIC_LABELS: string[] = ['记录', '字数', '时长'];
/** iOS chart `.frame(height: 112)`. */
const BAR_MAX: number = 112;
const BAR_GAP: number = 2;
/** iOS `.cornerRadius(3)`. */
const BAR_RADIUS: number = 3;
/** iOS title2 is 22pt semibold; the shared scale stops at 20 (title3). */
const TOTAL_SIZE: number = 22;
const TOTAL_LEADING: number = 28;
const METRIC_DURATION: number = 2;

@Component
export struct ActivityTrendCard {
  @Prop buckets: ActivityBucket[] = [];
  @Prop periodDays: number = 7;
  @Prop periodMetric: number = 0;
  @Prop periodMax: number = 0;
  @Prop total: string = '0';
  @Prop loaded: boolean = false;
  /** Supplied by the page: the card owns no application state. */
  @Prop reduceMotion: boolean = false;
  /** Loaded, but the selected window has no activity at all. */
  @Prop empty: boolean = false;
  onPeriod: (days: number) => void = (days: number) => {
  };
  onMetric: (metric: number) => void = (metric: number) => {
  };

  private hasData(): boolean {
    return this.loaded && this.periodMax > 0;
  }

  private axisTop(): string {
    return this.periodMax.toString();
  }

  private axisMid(): string {
    return Math.round(this.periodMax / 2).toString();
  }

  private barHeight(value: number): number {
    if (value <= 0 || this.periodMax <= 0) return 3;
    return Math.max(4, value / this.periodMax * (BAR_MAX - 6));
  }

  private dateLabel(date: string): string {
    return date.substring(5).replace('-', '/');
  }

  /** Up to four evenly spaced date marks (iOS strides the axis by 2 or 7 days). */
  private axisLabels(): string[] {
    const labels: string[] = [];
    const count: number = this.buckets.length;
    if (count === 0) return labels;
    const marks: number = count > 7 ? 4 : Math.min(count, 4);
    for (let index = 0; index < marks; index++) {
      const at: number = Math.round(index * (count - 1) / (marks - 1));
      labels.push(this.dateLabel(this.buckets[at].date));
    }
    return labels;
  }

  @Builder
  metricMenu() {
    Menu() {
      ForEach(METRIC_LABELS, (label: string, index: number) => {
        MenuItem({ content: label })
          .selected(index === this.periodMetric)
          .onChange(() => {
            this.onMetric(index);
          })
      }, (label: string) => 'metric-' + label)
    }
  }

  private caption(): string {
    return this.periodMetric === METRIC_DURATION ? '最近的使用趋势图表单位：分钟。' : '最近的使用趋势';
  }

  build() {
    Surface({ inset: Space.md }) {
      Column({ space: Space.md }) {
        Segmented({
          items: PERIOD_LABELS,
          selected: this.periodDays === 7 ? 0 : 1,
          onSelect: (index: number) => {
            this.onPeriod(index === 0 ? 7 : 30);
          }
        })

        Row() {
          Text(this.loaded ? this.total : '—')
            .fontSize(TOTAL_SIZE)
            .lineHeight(TOTAL_LEADING)
            .fontWeight(FontWeight.Medium)
            .fontColor(Palette.text1())
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
            .layoutWeight(1)
          Button({ type: ButtonType.Normal }) {
            Row({ space: Space.xxs }) {
              Text(METRIC_LABELS[this.periodMetric])
                .fontSize(Type.caption)
                .lineHeight(Leading.caption)
                .fontColor(Palette.accent())
              SymbolGlyph(symbolOf('chevron_down'))
                .fontSize(Type.caption)
                .fontColor([Palette.accent()])
            }
          }
          .backgroundColor(Color.Transparent)
          .accessibilityText('统计指标：' + METRIC_LABELS[this.periodMetric])
          .bindMenu(this.metricMenu())
        }
        .width('100%')
        .alignItems(VerticalAlign.Center)

        // Plot area: fixed 112vp like iOS `.frame(height: 112)`. Row height is
        // pinned explicitly so no child can stretch the card.
        Row({ space: Space.xs }) {
          Column() {
            Text(this.axisTop())
              .fontSize(Type.caption2).lineHeight(Leading.caption2).fontColor(Palette.text2())
            Blank()
            Text(this.axisMid())
              .fontSize(Type.caption2).lineHeight(Leading.caption2).fontColor(Palette.text2())
            Blank()
            Text('0')
              .fontSize(Type.caption2).lineHeight(Leading.caption2).fontColor(Palette.text2())
          }
          .width(26)
          .height(BAR_MAX)
          .alignItems(HorizontalAlign.End)
          .justifyContent(FlexAlign.SpaceBetween)
          .visibility(this.hasData() ? Visibility.Visible : Visibility.Hidden)

          Stack({ alignContent: Alignment.Center }) {
            Row({ space: BAR_GAP }) {
              ForEach(this.buckets, (bucket: ActivityBucket) => {
                Column() {
                  Column()
                    .width('100%')
                    .height(this.loaded ? this.barHeight(bucket.value) : 3)
                    .backgroundColor(Palette.accent())
                    .borderRadius(BAR_RADIUS)
                    .opacity(bucket.value > 0 ? 1 : 0.16)
                    .animation({
                      duration: this.reduceMotion ? Motion.reduced : 320,
                      curve: Curve.EaseOut
                    })
                }
                .layoutWeight(1)
                .height(BAR_MAX)
                .justifyContent(FlexAlign.End)
                .accessibilityText(this.dateLabel(bucket.date) + '，' + bucket.value.toString())
              }, (bucket: ActivityBucket) => 'bar-' + bucket.date)
            }
            .width('100%')
            .height(BAR_MAX)
            .alignItems(VerticalAlign.Bottom)

            if (!this.hasData()) {
              Text('开始录音后，这里会显示趋势')
                .fontSize(Type.caption)
                .lineHeight(Leading.caption)
                .fontColor(Palette.text2())
            }
          }
          .layoutWeight(1)
          .height(BAR_MAX)
        }
        .width('100%')
        .height(BAR_MAX)
        .alignItems(VerticalAlign.Bottom)

        Row() {
          ForEach(this.axisLabels(), (label: string) => {
            Text(label)
              .fontSize(Type.caption2)
              .lineHeight(Leading.caption2)
              .fontColor(Palette.text2())
              .layoutWeight(1)
          }, (label: string) => 'axis-' + label)
        }
        .width('100%')
        .padding({ left: this.hasData() ? 26 + Space.xs : 0 })

        Text(this.caption())
          .fontSize(Type.caption)
          .lineHeight(Leading.caption)
          .fontColor(Palette.text2())
          .width('100%')
      }
      .width('100%')
      .alignItems(HorizontalAlign.Start)
    }
  }
}
