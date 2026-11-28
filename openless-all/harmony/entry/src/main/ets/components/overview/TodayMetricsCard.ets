import { Fmt } from '../../common/Format';
import { Leading, Palette, Radius, Space, Type } from '../../common/Theme';

/**
 * iOS OverviewMetricsView.swift:64-97 (`metricsCard` + `tile`):
 *
 *   VStack(spacing: 0) {
 *     HStack(spacing: 0) { tile; Divider(); tile }
 *     Divider()
 *     HStack(spacing: 0) { tile; Divider(); tile }
 *   }
 *   .background(surface, RoundedRectangle(cornerRadius: 18))
 *   .overlay(strokeBorder(outline, 0.5))
 *
 * One tile: caption label, title3 semibold value, caption2 detail, 16pt padding.
 * The vertical divider is the left tile's right hairline so it always matches the
 * row height without a fixed number.
 */
@Component
struct OverviewMetricTile {
  @Prop title: string = '';
  @Prop metricId: string = '';
  @Prop value: string = '—';
  @Prop detail: string = '';
  @Prop divider: boolean = false;

  build() {
    Column({ space: Space.xs }) {
      Text(this.title)
        .fontSize(Type.caption)
        .lineHeight(Leading.caption)
        .fontColor(Palette.text2())
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
      Text(this.value)
        .fontSize(Type.section)
        .lineHeight(Leading.section)
        .fontWeight(FontWeight.Medium)
        .fontColor(Palette.text1())
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
      Text(this.detail)
        .fontSize(Type.caption2)
        .lineHeight(Leading.caption2)
        .fontColor(Palette.text2())
        .maxLines(2)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
    }
    .id(this.metricId)
    .width('100%')
    .alignItems(HorizontalAlign.Start)
    .padding(Space.md)
    .border({ width: { right: this.divider ? 0.5 : 0 }, color: Palette.line() })
  }
}

/** All values are supplied by the real activity / history APIs. */
@Component
export struct TodayMetricsCard {
  @Prop loaded: boolean = false;
  @Prop todayChars: number = 0;
  @Prop todayDuration: number = 0;
  @Prop todayCount: number = 0;
  @Prop totalRecords: number = 0;

  private characters(): string {
    return this.loaded ? Fmt.count(this.todayChars) : '—';
  }

  /** Zero stays a real zero ("0:00"), not the "—" used while the data is missing. */
  private durationText(milliseconds: number): string {
    if (!this.loaded) return '—';
    return milliseconds > 0 ? Fmt.duration(milliseconds) : '0:00';
  }

  private duration(): string {
    return this.durationText(this.todayDuration);
  }

  private average(): string {
    return this.durationText(this.todayCount > 0 ? Math.floor(this.todayDuration / this.todayCount) : 0);
  }

  private records(): string {
    return this.loaded ? Fmt.count(this.totalRecords) : '—';
  }

  build() {
    Column() {
      Flex({ direction: FlexDirection.Row, alignItems: ItemAlign.Stretch }) {
        OverviewMetricTile({
          metricId: 'metric-today-chars',
          title: '今日字数',
          value: this.characters(),
          detail: '今天写下的文字',
          divider: true
        }).layoutWeight(1)
        OverviewMetricTile({
          metricId: 'metric-today-duration',
          title: '今日时长',
          value: this.duration(),
          detail: this.todayCount.toString() + ' 段记录'
        }).layoutWeight(1)
      }
      .width('100%')

      Divider()
        .color(Palette.line())
        .strokeWidth(0.5)

      Flex({ direction: FlexDirection.Row, alignItems: ItemAlign.Stretch }) {
        OverviewMetricTile({
          metricId: 'metric-average-duration',
          title: '平均段落',
          value: this.average(),
          detail: '今日每段平均时长',
          divider: true
        }).layoutWeight(1)
        OverviewMetricTile({
          metricId: 'metric-total-records',
          title: '累计记录',
          value: this.records(),
          detail: '当前保留的历史'
        }).layoutWeight(1)
      }
      .width('100%')
    }
    .width('100%')
    .backgroundColor(Palette.card())
    .borderRadius(Radius.card)
    .border({ width: 0.5, color: Palette.line() })
    .clip(true)
  }
}
