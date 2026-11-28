import { Leading, Palette, Space, Type } from '../../common/Theme';
import { Surface, TextButton } from '../Kit';

/**
 * iOS `HistoryDetailView.textCard` (HistoryView.swift lines 225-236):
 * a product card with a semibold subheadline title, a trailing 复制 button and the
 * selectable 17pt body with `lineSpacing(6)`.
 *
 * Presentational only: the page supplies the text and the clipboard action.
 */
const CARD_GAP: number = 14;
/** iOS `lineSpacing(6)` on top of the 17pt body line height. */
const CARD_TEXT_LEADING: number = Leading.body + 6;

@Component
export struct HistoryTextCard {
  @Prop title: string = '';
  @Prop text: string = '';
  onCopy: () => void = () => {
  };

  build() {
    Surface({ inset: Space.md }) {
      Column({ space: CARD_GAP }) {
        Row({ space: Space.xs }) {
          Text(this.title)
            .fontSize(Type.label)
            .lineHeight(Leading.label)
            .fontWeight(FontWeight.Medium)
            .fontColor(Palette.text1())
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
            .layoutWeight(1)
            .textAlign(TextAlign.Start)
          TextButton({
            label: '复制',
            symbol: 'copy',
            onTap: () => {
              this.onCopy();
            }
          })
        }
        .width('100%')
        .alignItems(VerticalAlign.Center)

        Text(this.text)
          .fontSize(Type.body)
          .lineHeight(CARD_TEXT_LEADING)
          .fontColor(Palette.text1())
          .copyOption(CopyOptions.LocalDevice)
          .width('100%')
          .textAlign(TextAlign.Start)
      }
      .width('100%')
      .alignItems(HorizontalAlign.Start)
    }
  }
}
