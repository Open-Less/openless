import { Leading, Motion, Palette, Radius, Size, Space, Type } from '../../common/Theme';
import { symbolOf } from '../Icons';
import { activateOnKey } from '../Kit';

/**
 * iOS settings rows that the shared kit does not cover.
 *
 * Every component here is presentational: values arrive as `@Prop`, actions leave
 * through a callback. No backend state, preference document or Api call is held
 * here, so the page stays the single owner of data.
 */

/**
 * `AccountProfileHeader(compact:)` from iOS: a 44pt avatar, the account name in
 * headline weight and a secondary status line. Not a navigation link on Harmony,
 * where cloud backup has no separate account screen.
 */
@Component
export struct SettingsProfileRow {
  @Prop symbol: string = 'person';
  @Prop title: string;
  @Prop status: string = '';
  @Prop showDivider: boolean = false;
  onTap: () => void = () => {
  };

  build() {
    Column() {
      Row({ space: Space.sm }) {
        Stack() {
          SymbolGlyph(symbolOf(this.symbol))
            .fontSize(Size.icon)
            .fontColor([Palette.accent()])
        }
        .width(Size.touch)
        .height(Size.touch)
        .backgroundColor(Palette.accentSoft())
        .borderRadius(Radius.pill)
        .border({ width: 0.5, color: Palette.line() })

        Column({ space: 2 }) {
          Text(this.title)
            .fontSize(Type.cardTitle)
            .lineHeight(Leading.cardTitle)
            .fontWeight(FontWeight.Medium)
            .fontColor(Palette.text1())
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
          if (this.status.length > 0) {
            Text(this.status)
              .fontSize(Type.caption)
              .lineHeight(Leading.caption)
              .fontColor(Palette.text2())
              .maxLines(2)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
          }
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)
      }
      .width('100%')
      .constraintSize({ minHeight: 56 })
      .padding({ left: Space.md, right: Space.md, top: Space.xs, bottom: Space.xs })
      .hoverEffect(HoverEffect.Highlight)
      .focusable(true)
      .onKeyEvent((event: KeyEvent) => activateOnKey(event, () => this.onTap()))
      .onClick(() => this.onTap())

      if (this.showDivider) {
        Divider()
          .color(Palette.line())
          .strokeWidth(0.5)
          .margin({ left: Space.md + Size.touch + Space.sm })
      }
    }
    .width('100%')
  }
}

/**
 * A plain action row, the way iOS renders `Button("配置翻译")` inside a Section:
 * accent label, 44pt tall, hairline separator. No icon tile, because iOS action
 * rows are text-only; data rows keep their 28pt tile.
 */
@Component
export struct SettingsActionRow {
  @Prop label: string;
  @Prop value: string = '';
  @Prop tone: string = 'accent';
  @Prop usable: boolean = true;
  @Prop showDivider: boolean = true;
  onTap: () => void = () => {
  };

  private ink(): Resource {
    if (!this.usable) return Palette.text3();
    if (this.tone === 'quiet') return Palette.text2();
    return Palette.accent();
  }

  build() {
    Column() {
      Row({ space: Space.sm }) {
        Text(this.label)
          .fontSize(Type.body)
          .lineHeight(Leading.body)
          .fontColor(this.ink())
          .layoutWeight(1)
          .maxLines(2)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
        if (this.value.length > 0) {
          Text(this.value)
            .fontSize(Type.label)
            .lineHeight(Leading.label)
            .fontColor(Palette.text2())
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
        }
      }
      .width('100%')
      .constraintSize({ minHeight: Size.touch })
      .padding({ left: Space.md, right: Space.md })
      .hoverEffect(HoverEffect.Highlight)
      .focusable(true)
      .accessibilityText(this.label)
      .onKeyEvent((event: KeyEvent) => activateOnKey(event, () => {
        if (this.usable) this.onTap();
      }))
      .onClick(() => {
        if (this.usable) this.onTap();
      })

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
 * Destructive row: iOS renders a destructive `Button` of an inset-grouped list as
 * centred red text. The page keeps the confirmation sheet, this row only asks.
 */
@Component
export struct SettingsDangerRow {
  @Prop label: string;
  @Prop usable: boolean = true;
  @Prop showDivider: boolean = false;
  onTap: () => void = () => {
  };

  build() {
    Column() {
      Row() {
        Text(this.label)
          .fontSize(Type.body)
          .lineHeight(Leading.body)
          .fontColor(this.usable ? Palette.danger() : Palette.text3())
          .textAlign(TextAlign.Center)
          .width('100%')
          .maxLines(2)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
      }
      .width('100%')
      .constraintSize({ minHeight: Size.touch })
      .padding({ left: Space.md, right: Space.md })
      .hoverEffect(HoverEffect.Highlight)
      .focusable(true)
      .accessibilityText(this.label)
      .onKeyEvent((event: KeyEvent) => activateOnKey(event, () => {
        if (this.usable) this.onTap();
      }))
      .onClick(() => {
        if (this.usable) this.onTap();
      })

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
 * Microphone level test: the real input meter (`micLevel` storage link) next to
 * the row that starts and stops the monitor. The level is a prop, the toggle is a
 * callback; the monitor itself stays in the page.
 */
@Component
export struct SettingsMeterRow {
  @Prop title: string;
  @Prop hint: string = '';
  @Prop label: string = '';
  @Prop level: number = 0;
  @Prop testing: boolean = false;
  @Prop showDivider: boolean = true;
  onTap: () => void = () => {
  };

  private barWidth(): number {
    const clamped: number = Math.min(1, Math.max(0, this.level));
    return Math.round(Math.max(4, clamped * 96));
  }

  build() {
    Column() {
      Row({ space: Space.sm }) {
        Column({ space: 2 }) {
          Text(this.title)
            .fontSize(Type.body)
            .lineHeight(Leading.body)
            .fontColor(Palette.text1())
          if (this.hint.length > 0) {
            Text(this.hint)
              .fontSize(Type.caption)
              .lineHeight(Leading.caption)
              .fontColor(Palette.text2())
              .maxLines(2)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
          }
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)

        if (this.testing) {
          Stack({ alignContent: Alignment.Start }) {
            Row()
              .width(96)
              .height(6)
              .backgroundColor(Palette.control())
              .borderRadius(Radius.pill)
            Row()
              .width(this.barWidth())
              .height(6)
              .backgroundColor(Palette.accent())
              .borderRadius(Radius.pill)
              .animation({ duration: Motion.quick, curve: Curve.EaseOut })
          }
          .width(96)
          .height(6)
        }

        if (this.label.length > 0) {
          Text(this.label)
            .fontSize(Type.label)
            .lineHeight(Leading.label)
            .fontWeight(FontWeight.Medium)
            .fontColor(Palette.accent())
        }
      }
      .width('100%')
      .constraintSize({ minHeight: Size.touch })
      .padding({ left: Space.md, right: Space.md, top: Space.xs, bottom: Space.xs })
      .hoverEffect(HoverEffect.Highlight)
      .focusable(true)
      .accessibilityText(this.title)
      .onKeyEvent((event: KeyEvent) => activateOnKey(event, () => this.onTap()))
      .onClick(() => this.onTap())

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
 * The trailing version line of iOS Settings: one card whose only content is a
 * centred 13pt grey label. Harmony has no developer-mode unlock behind it, so it
 * stays a read-only footnote instead of a hidden tap target.
 */
@Component
export struct SettingsFootnoteRow {
  @Prop label: string;

  build() {
    Row() {
      Text(this.label)
        .fontSize(Type.caption)
        .lineHeight(Leading.caption)
        .fontColor(Palette.text3())
        .textAlign(TextAlign.Center)
        .width('100%')
    }
    .width('100%')
    .constraintSize({ minHeight: Size.touch })
    .padding({ left: Space.md, right: Space.md })
  }
}
