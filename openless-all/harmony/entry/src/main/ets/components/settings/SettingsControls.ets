import { Leading, Palette, Radius, Size, Space, Type } from '../../common/Theme';
import { activateOnKey, Segmented } from '../Kit';
import { symbolOf } from '../Icons';

/**
 * iOS renders a `Picker` row as icon + title + trailing value. HarmonyOS has no
 * menu picker in this kit, so the same row keeps the 44pt title line and puts the
 * native segmented control (the iOS segmented style the kit already owns)
 * underneath it, inside the card.
 */
@Component
export struct SettingsChoiceRow {
  @Prop title: string;
  @Prop hint: string = '';
  @Prop items: string[] = [];
  @Prop selected: number = 0;
  @Prop showDivider: boolean = true;
  onSelect: (index: number) => void = (index: number) => {
  };

  build() {
    Column() {
      Column({ space: Space.xs }) {
        Text(this.title)
          .fontSize(Type.body)
          .lineHeight(Leading.body)
          .fontColor(Palette.text1())
          .width('100%')
        if (this.hint.length > 0) {
          Text(this.hint)
            .fontSize(Type.caption)
            .lineHeight(Leading.caption)
            .fontColor(Palette.text2())
            .width('100%')
        }
        Segmented({
          items: this.items,
          selected: this.selected,
          onSelect: (index: number) => this.onSelect(index)
        })
      }
      .width('100%')
      .alignItems(HorizontalAlign.Start)
      .padding({ left: Space.md, right: Space.md, top: Space.sm, bottom: Space.sm })

      if (this.showDivider) {
        Divider()
          .color(Palette.line())
          .strokeWidth(0.5)
          .margin({ left: Space.md })
      }
    }
    .width('100%')
  }
}

/**
 * Wide-window group index: the left column of a two-pane Settings page. Rows keep
 * the list-row rhythm (44pt, 28pt tile) and mark the active group with the accent
 * tint instead of a large page title.
 */
@Component
export struct SettingsNavRow {
  @Prop title: string;
  @Prop symbol: string = '';
  @Prop selected: boolean = false;
  @Prop showDivider: boolean = true;
  onTap: () => void = () => {
  };

  build() {
    Column() {
      Row({ space: Space.sm }) {
        if (this.symbol.length > 0) {
          Stack() {
            SymbolGlyph(symbolOf(this.symbol))
              .fontSize(14)
              .fontColor([this.selected ? Palette.accent() : Palette.icon2()])
          }
          .width(28)
          .height(28)
          .backgroundColor(this.selected ? Palette.card() : Palette.cardAlt())
          .borderRadius(Radius.icon)
        }

        Text(this.title)
          .fontSize(Type.body)
          .lineHeight(Leading.body)
          .fontColor(this.selected ? Palette.accent() : Palette.text1())
          .fontWeight(this.selected ? FontWeight.Medium : FontWeight.Normal)
          .layoutWeight(1)
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })

        if (this.selected) {
          SymbolGlyph(symbolOf('check'))
            .fontSize(Size.iconSmall)
            .fontColor([Palette.accent()])
        }
      }
      .width('100%')
      .constraintSize({ minHeight: Size.touch })
      .padding({ left: Space.xs, right: Space.sm })
      .backgroundColor(this.selected ? Palette.accentSoft() : Color.Transparent)
      .borderRadius(Radius.control)
      .hoverEffect(HoverEffect.Highlight)
      .focusable(true)
      .accessibilityText(this.title)
      .accessibilitySelected(this.selected)
      .onKeyEvent((event: KeyEvent) => activateOnKey(event, () => this.onTap()))
      .onClick(() => this.onTap())

      if (this.showDivider) {
        Divider()
          .color(Palette.line())
          .strokeWidth(0.5)
          .margin({ left: Space.sm })
      }
    }
    .width('100%')
  }
}
