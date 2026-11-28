import { Leading, Palette, Radius, Size, Space, Type } from '../../common/Theme';
import { symbolOf } from '../Icons';

/**
 * iOS HomeView.swift:119-129, one-to-one:
 *
 *   HStack(spacing: 10) {
 *     OpenLessMark(size: 34)
 *     Text("OpenLess").font(.system(size: 22, weight: .semibold)).tracking(-0.6)
 *     Spacer()
 *     NavigationLink(keyboardSetup) { keyboard.badge.ellipsis in a 44pt accentSoft circle }
 *   }
 *
 * Reuse the shipped OpenLess brand asset rather than a placeholder monogram.
 * The keyboard button opens the real input-method/settings surface; it does not
 * pretend to install or enable a keyboard without system authorization.
 */

/** Mark edge, iOS OpenLessMark(size: 34). */
const BRAND_MARK: number = 34;
/** iOS brand wordmark is 22pt semibold; the shared scale stops at 20 (title3). */
const BRAND_TITLE: number = 22;
const BRAND_LEADING: number = 28;
/** iOS tracking(-0.6). */
const BRAND_TRACKING: number = -0.6;

@Component
export struct BrandBar {
  onKeyboard: () => void = () => {
  };

  build() {
    Row({ space: Space.sm }) {
      Image($r('app.media.openless_icon'))
        .width(BRAND_MARK)
        .height(BRAND_MARK)
        .objectFit(ImageFit.Contain)
        .borderRadius(BRAND_MARK * 0.22)
        .accessibilityLevel('no')

      Text('OpenLess')
        .fontSize(BRAND_TITLE)
        .lineHeight(BRAND_LEADING)
        .fontWeight(FontWeight.Medium)
        .fontColor(Palette.text1())
        .letterSpacing(BRAND_TRACKING)

      Blank()

      Button({ type: ButtonType.Circle }) {
        SymbolGlyph(symbolOf('keyboard'))
          .fontSize(Type.navTitle)
          .fontColor([Palette.accent()])
      }
      .width(Size.touch)
      .height(Size.touch)
      .backgroundColor(Palette.accentSoft())
      .hoverEffect(HoverEffect.Highlight)
      .accessibilityText('输入法与键盘')
      .onClick(() => this.onKeyboard())
    }
    .width('100%')
    .alignItems(VerticalAlign.Center)
  }
}
