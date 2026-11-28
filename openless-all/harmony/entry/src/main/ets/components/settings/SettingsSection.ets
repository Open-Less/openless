import { Leading, Palette, Space, Type } from '../../common/Theme';
import { Group } from '../Kit';

@Builder
function emptySection() {}

/**
 * One `Section` of an iOS inset-grouped list.
 *
 * iOS draws the group header as a 13pt secondary caption above the card (see
 * `AppDesign.SettingsCard`, which insets it by 6pt), and an optional 13pt
 * tertiary footer below it. The card itself keeps the kit `Group` (18pt radius,
 * hairline outline), so rows inside stay plain 44pt list rows.
 *
 * Presentational only: the caller owns preferences, filtering and callbacks.
 */
@Component
export struct SettingsSection {
  @Prop title: string = '';
  @Prop footer: string = '';
  @BuilderParam content: () => void = emptySection;

  build() {
    Column({ space: Space.xs }) {
      if (this.title.length > 0) {
        Text(this.title)
          .fontSize(Type.caption)
          .lineHeight(Leading.caption)
          .fontColor(Palette.text2())
          .width('100%')
          .padding({ left: 4 })
      }

      Group({ radius: 12 }) {
        this.content()
      }

      if (this.footer.length > 0) {
        Text(this.footer)
          .fontSize(Type.caption)
          .lineHeight(Leading.caption)
          .fontColor(Palette.text3())
          .width('100%')
          .padding({ left: 6 })
      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
  }
}
