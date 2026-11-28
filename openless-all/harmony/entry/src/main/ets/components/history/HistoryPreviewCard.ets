import { Leading, Palette, Radius, Space, Type } from '../../common/Theme';
import { activateOnKey } from '../Kit';

/**
 * One row of the iOS history list (`HistoryView.swift` lines 46-67), ported 1:1:
 *
 *   HStack(firstTextBaseline)  caption date ............ caption.monospacedDigit duration
 *   Text(displayText)          body, lineSpacing 4, lineLimit 3
 *   HStack(spacing: 6)         [exclamationmark.circle] caption status
 *   .productCard()             18 radius, 0.5 hairline, 16 inset
 *
 * Presentational only: the page owns entries, selection and formatting.
 */

/** iOS `VStack(spacing: 10)` inside the card. */
const CARD_GAP: number = 10;
/** iOS `lineSpacing(4)` on top of the 17pt body line height. */
const CARD_TEXT_LEADING: number = Leading.body + 4;
/** iOS `HStack(spacing: 6)` for the status line. */
const STATUS_GAP: number = 6;

@Component
export struct HistoryPreviewCard {
  @Prop timestamp: string = '';
  @Prop duration: string = '';
  @Prop text: string = '';
  @Prop status: string = '';
  @Prop failed: boolean = false;
  @Prop selected: boolean = false;
  onSelect: () => void = () => {
  };

  build() {
    Column({ space: CARD_GAP }) {
      Row({ space: Space.xs }) {
        Text(this.timestamp)
          .fontSize(Type.caption)
          .lineHeight(Leading.caption)
          .fontColor(Palette.text2())
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .layoutWeight(1)
          .textAlign(TextAlign.Start)
        if (this.duration.length > 0) {
          Text(this.duration)
            .fontSize(Type.caption)
            .lineHeight(Leading.caption)
            .fontColor(Palette.text2())
            .fontFeature('"tnum" 1')
            .maxLines(1)
        }
      }
      .width('100%')
      .alignItems(VerticalAlign.Center)

      Text(this.text.length > 0 ? this.text : '未生成文字，点按查看详情')
        .fontSize(Type.body)
        .lineHeight(CARD_TEXT_LEADING)
        .fontColor(Palette.text1())
        .maxLines(3)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
        .width('100%')
        .textAlign(TextAlign.Start)

      Row({ space: STATUS_GAP }) {
        if (this.failed) {
          // iOS `Image(systemName: "exclamationmark.circle")`.
          Stack() {
            Text('!')
              .fontSize(Type.caption2)
              .fontWeight(FontWeight.Bold)
              .fontColor(Palette.danger())
          }
          .width(15)
          .height(15)
          .borderRadius(Radius.pill)
          .border({ width: 1, color: Palette.danger() })
        }
        Text(this.status)
          .fontSize(Type.caption)
          .lineHeight(Leading.caption)
          .fontColor(this.failed ? Palette.danger() : Palette.text2())
          .maxLines(2)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .layoutWeight(1)
          .textAlign(TextAlign.Start)
      }
      .width('100%')
      .alignItems(VerticalAlign.Center)
    }
    .width('100%')
    .padding(Space.md)
    .alignItems(HorizontalAlign.Start)
    .backgroundColor(this.selected ? Palette.accentSoft() : Palette.card())
    .borderRadius(Radius.card)
    .border({ width: 0.5, color: this.selected ? Palette.accent() : Palette.line() })
    .hoverEffect(HoverEffect.Highlight)
    .focusable(true)
    .onKeyEvent((event: KeyEvent) => activateOnKey(event, () => this.onSelect()))
    .onClick(() => this.onSelect())
    .accessibilityGroup(true)
    .accessibilityText(this.timestamp + '，' + this.text + '，' + this.status)
  }
}
