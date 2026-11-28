import { Leading, Palette, Radius, Size, Space, Type } from '../../common/Theme';
import { TextButton } from '../Kit';
import { symbolOf } from '../Icons';

/**
 * iOS HomeView.swift:133-145: the failure card sits directly under the voice card,
 * as its own product card. `danger` is the Harmony surface for it (iOS keeps the
 * neutral card with a plain label; the design tokens here name an error card).
 *
 * The 「前往系统设置」 row only appears when the microphone permission itself is
 * missing — same condition as `speechAuthorizationDenied` on iOS. It calls the real
 * `open_system_settings` bridge command, never a placeholder.
 */
@Component
export struct SpeechErrorCard {
  @Prop message: string = '';
  @Prop denied: boolean = false;
  onSettings: () => void = () => {
  };

  build() {
    Column({ space: Space.sm }) {
      Row({ space: Space.xs }) {
        SymbolGlyph(symbolOf('close'))
          .fontSize(Size.iconSmall)
          .fontColor([Palette.danger()])
        Text(this.denied ? '请允许麦克风访问' : '语音暂时不可用，请重试。')
          .fontSize(Type.caption)
          .lineHeight(Leading.caption)
          .fontColor(Palette.text1())
          .layoutWeight(1)
      }
      .width('100%')
      .alignItems(VerticalAlign.Top)

      if (this.message.length > 0) {
        Text(this.message)
          .fontSize(Type.caption2)
          .lineHeight(Leading.caption2)
          .fontColor(Palette.text2())
          .width('100%')
      }

      if (this.denied) {
        Row() {
          TextButton({
            label: '前往系统设置',
            tone: 'danger',
            onTap: () => {
              this.onSettings();
            }
          })
        }
        .width('100%')
      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
    .padding(Space.md)
    .backgroundColor(Palette.dangerSoft())
    .borderRadius(Radius.card)
  }
}
