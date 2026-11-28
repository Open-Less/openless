import { KeyCode } from '@kit.InputKit';
import { promptAction } from '@kit.ArkUI';
import { Layout, Leading, Palette, Radius, Size, Space, Type } from '../common/Theme';
import { symbolOf } from './Icons';

/**
 * v2 component kit.
 *
 * Every visual primitive of the main app lives here: pages compose these and must
 * not re-implement buttons, rows, chips or cards inline. Keeping the vocabulary
 * small is what makes the pages look like one product.
 */

/**
 * Activate a custom clickable surface from a physical keyboard. Tablet and PC
 * users navigate by focus, so every row/card answers Enter/Space.
 * https://developer.huawei.com/consumer/cn/doc/best-practices-V14/bpta-multi-interaction-V14
 */
export function activateOnKey(event: KeyEvent, action: () => void): void {
  if (event.type !== KeyType.Down) return;
  if (event.keyCode === KeyCode.KEYCODE_ENTER || event.keyCode === KeyCode.KEYCODE_NUMPAD_ENTER ||
    event.keyCode === KeyCode.KEYCODE_SPACE) {
    event.stopPropagation();
    action();
  }
}

/** A nested native Button must not also activate an enclosing clickable card. */
export function stopButtonKeyBubbling(event: KeyEvent): void {
  if (event.keyCode === KeyCode.KEYCODE_ENTER || event.keyCode === KeyCode.KEYCODE_NUMPAD_ENTER ||
    event.keyCode === KeyCode.KEYCODE_SPACE) event.stopPropagation();
}

/** Section heading inside a card. */
@Component
export struct SectionTitle {
  @Prop title: string;
  @Prop hint: string = '';
  @Prop action: string = '';
  onAction: () => void = () => {
  };

  build() {
    Row({ space: Space.sm }) {
      Column({ space: Space.xxs }) {
        Text(this.title)
          .fontSize(Type.label)
          .lineHeight(Leading.label)
          .fontWeight(FontWeight.Medium)
          .fontColor(Palette.text1())
        if (this.hint.length > 0) {
          Text(this.hint)
            .fontSize(Type.caption)
            .lineHeight(Leading.caption)
            .fontColor(Palette.text2())
        }
      }
      .alignItems(HorizontalAlign.Start)
      .layoutWeight(1)

      if (this.action.length > 0) {
        TextButton({ label: this.action, onTap: () => this.onAction() })
      }
    }
    .width('100%')
  }
}

/** White card container. `tone`: card | alt | plain. */
@Component
export struct Surface {
  @Prop tone: string = 'card';
  @Prop inset: number = Space.md;
  @BuilderParam content: () => void;

  private fill(): Resource {
    if (this.tone === 'alt') return Palette.cardAlt();
    if (this.tone === 'plain') return Palette.bg();
    return Palette.card();
  }

  build() {
    Column() {
      this.content()
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
    .padding(this.inset)
    .backgroundColor(this.fill())
    .borderRadius(Radius.card)
    .border({ width: 0.5, color: Palette.line() })
  }
}

/** Grouped list container: rows inside keep their own padding and dividers. */
@Component
export struct Group {
  @Prop fill: string = 'card';
  /** iOS inset-grouped default is tighter than the 18vp product card. */
  @Prop radius: number = Radius.card;
  @BuilderParam content: () => void;

  build() {
    Column() {
      this.content()
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
    .backgroundColor(this.fill === 'alt' ? Palette.cardAlt() : Palette.card())
    .borderRadius(this.radius)
    .border({ width: 0.5, color: Palette.line() })
    .clip(true)
  }
}

/** Standard list row: optional icon tile, title/subtitle, trailing value/chevron. */
@Component
export struct RowItem {
  @Prop symbol: string = '';
  @Prop accentSymbol: boolean = false;
  @Prop title: string;
  @Prop subtitle: string = '';
  @Prop value: string = '';
  @Prop showChevron: boolean = false;
  @Prop showDivider: boolean = true;
  @Prop muted: boolean = false;
  /** iOS menu pickers render their current value in the accent colour. */
  @Prop valueAccent: boolean = false;
  onTap: () => void = () => {
  };

  build() {
    Column() {
      Row({ space: Space.sm }) {
        if (this.symbol.length > 0) {
          Stack() {
            SymbolGlyph(symbolOf(this.symbol))
              .fontSize(14)
              .fontColor([Palette.accent()])
          }
          .width(28)
          .height(28)
          .backgroundColor(Palette.accentSoft())
          .borderRadius(Radius.icon)
        }

        Column({ space: Space.xxs }) {
          Text(this.title)
            .fontSize(Type.body)
            .lineHeight(Leading.body)
            .fontColor(Palette.text1())
            .maxLines(2)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
          if (this.subtitle.length > 0) {
            Text(this.subtitle)
              .fontSize(Type.caption)
              .lineHeight(Leading.caption)
              .fontColor(Palette.text2())
              .maxLines(2)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
          }
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)

        if (this.value.length > 0) {
          Text(this.value)
            .fontSize(Type.label)
            .lineHeight(Leading.label)
            .fontColor(this.valueAccent ? Palette.accent() : Palette.text2())
            .textAlign(TextAlign.End)
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
            .constraintSize({ maxWidth: 180 })
        }
        if (this.showChevron) {
          SymbolGlyph(symbolOf('chevron'))
            .fontSize(Size.iconSmall)
            .fontColor([Palette.text3()])
        }
      }
      .width('100%')
      .constraintSize({ minHeight: this.subtitle.length > 0 ? 60 : Size.touch })
      .padding({ left: Space.md, right: Space.md, top: Space.xs, bottom: Space.xs })
      .opacity(this.muted ? 0.45 : 1)
      .hoverEffect(HoverEffect.Highlight)
      .focusable(true)
      .onKeyEvent((event: KeyEvent) => activateOnKey(event, () => this.onTap()))
      .onClick(() => this.onTap())

      if (this.showDivider) {
        Divider()
          .color(Palette.line())
          .strokeWidth(0.5)
          .margin({ left: this.symbol.length > 0 ? Space.md + 28 + Space.sm : Space.md })
      }
    }
    .width('100%')
  }
}

/** Filled action button. */
@Component
export struct PrimaryButton {
  @Prop label: string;
  @Prop usable: boolean = true;
  @Prop destructive: boolean = false;
  onTap: () => void = () => {
  };

  build() {
    Button(this.label, { type: ButtonType.Capsule })
      .constraintSize({ minHeight: Size.touch })
      .fontSize(Type.body)
      .fontWeight(FontWeight.Medium)
      .fontColor(Palette.onAccent())
      .backgroundColor(this.destructive ? Palette.danger() : Palette.accent())
      .padding({ left: Space.lg, right: Space.lg })
      .opacity(this.usable ? 1 : 0.4)
      .enabled(this.usable)
      .stateStyles({
        normal: { .opacity(this.usable ? 1 : 0.4) },
        pressed: { .opacity(0.82) }
      })
      .onKeyEvent((event: KeyEvent) => stopButtonKeyBubbling(event))
      .onClick((event: ClickEvent) => {
        if (this.usable) this.onTap();
      })
  }
}

/** Tonal action button (accent-soft background). */
@Component
export struct TonalButton {
  @Prop label: string;
  @Prop symbol: string = '';
  @Prop usable: boolean = true;
  onTap: () => void = () => {
  };

  build() {
    Button({ type: ButtonType.Capsule }) {
      Row({ space: Space.xs }) {
        if (this.symbol.length > 0) {
          SymbolGlyph(symbolOf(this.symbol))
            .fontSize(Size.iconSmall)
            .fontColor([Palette.accent()])
        }
        Text(this.label)
          .fontSize(Type.label)
          .lineHeight(Leading.label)
          .fontWeight(FontWeight.Medium)
          .fontColor(Palette.accent())
      }
    }
    .constraintSize({ minHeight: Size.touch })
    .padding({ left: Space.md, right: Space.md })
    .backgroundColor(Palette.accentSoft())
    .borderRadius(Radius.pill)
    .opacity(this.usable ? 1 : 0.4)
    .enabled(this.usable)
    .hoverEffect(HoverEffect.Highlight)
    .accessibilityText(this.label)
    .onKeyEvent((event: KeyEvent) => stopButtonKeyBubbling(event))
    .onClick((event: ClickEvent) => {
      this.onTap();
    })
  }
}

/** Borderless text action (headers, inline links). */
@Component
export struct TextButton {
  @Prop label: string;
  @Prop symbol: string = '';
  @Prop tone: string = 'accent';
  @Prop usable: boolean = true;
  onTap: () => void = () => {
  };

  private ink(): Resource {
    if (this.tone === 'danger') return Palette.danger();
    if (this.tone === 'quiet') return Palette.text2();
    return Palette.accent();
  }

  build() {
    Button({ type: ButtonType.Normal }) {
      Row({ space: Space.xxs }) {
        if (this.symbol.length > 0) {
          SymbolGlyph(symbolOf(this.symbol))
            .fontSize(Size.iconSmall)
            .fontColor([this.ink()])
        }
        Text(this.label)
          .fontSize(Type.label)
          .lineHeight(Leading.label)
          .fontWeight(FontWeight.Medium)
          .fontColor(this.ink())
      }
    }
    .constraintSize({ minHeight: Size.touch })
    .padding({ left: Space.xs, right: Space.xs, top: Space.xxs, bottom: Space.xxs })
    .backgroundColor(Color.Transparent)
    .borderRadius(Radius.control)
    .opacity(this.usable ? 1 : 0.4)
    .enabled(this.usable)
    .hoverEffect(HoverEffect.Highlight)
    .accessibilityText(this.label)
    .onKeyEvent((event: KeyEvent) => stopButtonKeyBubbling(event))
    .onClick((event: ClickEvent) => {
      this.onTap();
    })
  }
}

/** Square icon-only action. */
@Component
export struct IconButton {
  @Prop symbol: string;
  @Prop tint: string = 'neutral';
  onTap: () => void = () => {
  };

  build() {
    Button({ type: ButtonType.Circle }) {
      SymbolGlyph(symbolOf(this.symbol))
        .fontSize(Size.icon)
        .fontColor([this.tint === 'accent' ? Palette.accent() : Palette.icon1()])
    }
    .width(Size.touch)
    .height(Size.touch)
    .backgroundColor(Palette.cardAlt())
    .hoverEffect(HoverEffect.Highlight)
    .accessibilityText(this.symbol)
    .onKeyEvent((event: KeyEvent) => stopButtonKeyBubbling(event))
    .onClick((event: ClickEvent) => {
      this.onTap();
    })
  }
}

/** Single-select segmented control. Wraps on narrow screens instead of clipping. */
@Component
export struct Segmented {
  @Prop items: string[] = [];
  @Prop selected: number = 0;
  onSelect: (index: number) => void = (index: number) => {
  };

  build() {
    Flex({ wrap: FlexWrap.Wrap, justifyContent: FlexAlign.Start }) {
      ForEach(this.items, (item: string, index: number) => {
        Button({ type: ButtonType.Normal }) {
          Text(item)
            .fontSize(Type.caption)
            .lineHeight(Leading.caption)
            .fontColor(index === this.selected ? Palette.text1() : Palette.text2())
            .fontWeight(index === this.selected ? FontWeight.Medium : FontWeight.Normal)
        }
        .constraintSize({ minHeight: 30 })
        .padding({ left: Space.sm, right: Space.sm })
        .margin(2)
        .backgroundColor(index === this.selected ? Palette.controlSelected() : Color.Transparent)
        .borderRadius(Radius.control)
        .hoverEffect(HoverEffect.Highlight)
        .accessibilityText(item)
        .accessibilitySelected(index === this.selected)
        .onClick(() => this.onSelect(index))
      }, (item: string) => item)
    }
    .width('100%')
    .padding(Space.xxs)
    .backgroundColor(Palette.control())
    .borderRadius(Radius.pill)
  }
}

/** Read-only status tag. */
@Component
export struct Tag {
  @Prop text: string;
  @Prop tone: string = 'neutral';

  private tint(): Resource {
    if (this.tone === 'accent') return Palette.accentSoft();
    if (this.tone === 'success') return Palette.successSoft();
    if (this.tone === 'warning') return Palette.warningSoft();
    if (this.tone === 'danger') return Palette.dangerSoft();
    return Palette.cardAlt();
  }

  private ink(): Resource {
    if (this.tone === 'accent') return Palette.accent();
    if (this.tone === 'success') return Palette.success();
    if (this.tone === 'warning') return Palette.warning();
    if (this.tone === 'danger') return Palette.danger();
    return Palette.text2();
  }

  build() {
    Text(this.text)
      .fontSize(Type.caption)
      .lineHeight(Leading.caption)
      .fontWeight(FontWeight.Medium)
      .fontColor(this.ink())
      .padding({ left: Space.xs, right: Space.xs, top: 2, bottom: 2 })
      .backgroundColor(this.tint())
      .borderRadius(Radius.pill)
  }
}

/** Search / filter input. */
@Component
export struct SearchField {
  @Prop placeholder: string = '搜索';
  @Prop value: string = '';
  onInput: (value: string) => void = (value: string) => {
  };

  build() {
    Row({ space: Space.xs }) {
      SymbolGlyph(symbolOf('search'))
        .fontSize(Size.iconSmall)
        .fontColor([Palette.text3()])
      TextInput({ placeholder: this.placeholder, text: this.value })
        .fontSize(Type.body)
        .fontColor(Palette.text1())
        .placeholderColor(Palette.text3())
        .backgroundColor(Color.Transparent)
        .padding(0)
        .layoutWeight(1)
        .onChange((text: string) => this.onInput(text))
      if (this.value.length > 0) {
        Button({ type: ButtonType.Circle }) {
          SymbolGlyph(symbolOf('close'))
            .fontSize(Size.iconSmall)
            .fontColor([Palette.text3()])
        }
        .width(32)
        .height(32)
        .backgroundColor(Color.Transparent)
        .accessibilityText('清除搜索内容')
        .onClick(() => this.onInput(''))
      }
    }
    .width('100%')
    .constraintSize({ minHeight: 40 })
    .padding({ left: Space.sm, right: Space.sm })
    .backgroundColor(Palette.cardAlt())
    .borderRadius(Radius.control)
  }
}

/** Empty state: symbol + one line + hint. */
@Component
export struct EmptyHint {
  @Prop symbol: string = 'sparkles';
  @Prop title: string;
  @Prop hint: string = '';

  build() {
    Column({ space: Space.sm }) {
      Stack() {
        SymbolGlyph(symbolOf(this.symbol))
          .fontSize(24)
          .fontColor([Palette.accent()])
      }
      .width(56)
      .height(56)
      .backgroundColor(Palette.accentSoft())
      .borderRadius(14)
      Text(this.title)
        .fontSize(Type.cardTitle)
        .lineHeight(Leading.cardTitle)
        .fontWeight(FontWeight.Medium)
        .fontColor(Palette.text1())
        .textAlign(TextAlign.Center)
      if (this.hint.length > 0) {
        Text(this.hint)
          .fontSize(Type.caption)
          .lineHeight(Leading.caption)
          .fontColor(Palette.text2())
          .textAlign(TextAlign.Center)
      }
    }
    .width('100%')
    .padding({ top: 48, bottom: 48, left: Space.lg, right: Space.lg })
    .justifyContent(FlexAlign.Center)
  }
}

/** Loading placeholder: a few neutral bars instead of a spinner over empty content. */
@Component
export struct Skeleton {
  @Prop rows: number = 3;

  build() {
    Column({ space: Space.sm }) {
      ForEach([0, 1, 2, 3, 4], (index: number) => {
        if (index < this.rows) {
          Row({ space: Space.sm }) {
            Column()
              .width(Size.tile)
              .height(Size.tile)
              .backgroundColor(Palette.cardAlt())
              .borderRadius(Radius.control)
            Column({ space: Space.xs }) {
              Column()
                .width('60%')
                .height(12)
                .backgroundColor(Palette.cardAlt())
                .borderRadius(Radius.control)
              Column()
                .width('35%')
                .height(10)
                .backgroundColor(Palette.cardAlt())
                .borderRadius(Radius.control)
            }
            .alignItems(HorizontalAlign.Start)
            .layoutWeight(1)
          }
          .width('100%')
          .padding(Space.sm)
          .backgroundColor(Palette.card())
          .borderRadius(Radius.card)
          .border({ width: 0.5, color: Palette.line() })
        }
      }, (index: number) => 'skeleton-' + index.toString())
    }
    .width('100%')
  }
}

/** Inline error with a retry action. */
@Component
export struct ErrorNote {
  @Prop message: string;
  onRetry: () => void = () => {
  };

  build() {
    Row({ space: Space.sm }) {
      SymbolGlyph(symbolOf('close'))
        .fontSize(Size.iconSmall)
        .fontColor([Palette.danger()])
      Text(this.message)
        .fontSize(Type.label)
        .lineHeight(Leading.label)
        .fontColor(Palette.text2())
        .layoutWeight(1)
      TextButton({ label: '重试', tone: 'quiet', onTap: () => this.onRetry() })
    }
    .width('100%')
    .padding(Space.sm)
    .backgroundColor(Palette.dangerSoft())
    .borderRadius(Radius.card)
  }
}

/** Soft note: onboarding, setup hints, warnings. */
@Component
export struct Banner {
  @Prop title: string;
  @Prop text: string = '';
  @Prop tone: string = 'accent';
  @Prop action: string = '';
  @Prop actionSymbol: string = '';
  onAction: () => void = () => {
  };

  private surface(): Resource {
    if (this.tone === 'warning') return Palette.warningSoft();
    if (this.tone === 'danger') return Palette.dangerSoft();
    return Palette.accentSoft();
  }

  private ink(): Resource {
    if (this.tone === 'warning') return Palette.warning();
    if (this.tone === 'danger') return Palette.danger();
    return Palette.accent();
  }

  build() {
    Column({ space: Space.xs }) {
      Text(this.title)
        .fontSize(Type.cardTitle)
        .lineHeight(Leading.cardTitle)
        .fontWeight(FontWeight.Medium)
        .fontColor(Palette.text1())
      if (this.text.length > 0) {
        Text(this.text)
          .fontSize(Type.label)
          .lineHeight(Leading.label)
          .fontColor(Palette.text2())
      }
      if (this.action.length > 0) {
        Row() {
          TonalButton({ label: this.action, symbol: this.actionSymbol, onTap: () => this.onAction() })
        }
        .width('100%')
        .padding({ top: Space.xxs })
      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
    .padding(Space.md)
    .backgroundColor(this.surface())
    .borderRadius(Radius.card)
  }
}

/** Big number with label. `size`: metric | section. */
@Component
export struct MetricBlock {
  @Prop value: string;
  @Prop label: string;
  @Prop unit: string = '';
  @Prop hero: boolean = false;

  build() {
    Column({ space: Space.xxs }) {
      Row({ space: Space.xxs }) {
        Text(this.value)
          .fontSize(this.hero ? Type.metric : Type.section)
          .lineHeight(this.hero ? Leading.metric : Leading.section)
          .fontWeight(FontWeight.Medium)
          .fontColor(Palette.text1())
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
        if (this.unit.length > 0) {
          Text(this.unit)
            .fontSize(Type.caption)
            .fontColor(Palette.text3())
            .margin({ bottom: 4 })
        }
      }
      .alignItems(VerticalAlign.Bottom)

      Text(this.label)
        .fontSize(Type.label)
        .lineHeight(Leading.label)
        .fontColor(Palette.text3())
    }
    .alignItems(HorizontalAlign.Start)
    .layoutWeight(1)
  }
}

/** Label + hint + switch. */
@Component
export struct SettingSwitch {
  @Prop label: string;
  @Prop hint: string = '';
  @Prop checked: boolean = false;
  @Prop showDivider: boolean = true;
  onToggle: (value: boolean) => void = (value: boolean) => {
  };

  build() {
    Column() {
      Row({ space: Space.md }) {
        Column({ space: Space.xxs }) {
          Text(this.label)
            .fontSize(Type.body)
            .lineHeight(Leading.body)
            .fontColor(Palette.text1())
          if (this.hint.length > 0) {
            Text(this.hint)
              .fontSize(Type.caption)
              .lineHeight(Leading.caption)
              .fontColor(Palette.text3())
          }
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)

        Toggle({ type: ToggleType.Switch, isOn: this.checked })
          .selectedColor(Palette.accent())
          .onChange((value: boolean) => this.onToggle(value))
      }
      .width('100%')
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

/** Back affordance for secondary pages. */
@Component
export struct PageBackBar {
  @Prop label: string;
  onTap: () => void = () => {
  };

  build() {
    Button({ type: ButtonType.Normal }) {
      Row({ space: Space.xxs }) {
        SymbolGlyph(symbolOf('chevron_left'))
          .fontSize(Size.icon)
          .fontColor([Palette.accent()])
        Text(this.label)
          .fontSize(Type.label)
          .lineHeight(Leading.label)
          .fontColor(Palette.accent())
      }
    }
    .constraintSize({ minHeight: Size.touch })
    .padding({ right: Space.sm })
    .backgroundColor(Color.Transparent)
    .hoverEffect(HoverEffect.Highlight)
    .accessibilityText('返回' + this.label)
    .onClick(() => this.onTap())
  }
}

/**
 * Page frame: large title + one header action + a body that either scrolls or
 * fills. Every page starts with it so titles, gutters and bottom insets line up.
 */
@Component
export struct PageScaffold {
  @Prop title: string;
  @Prop subtitle: string = '';
  /** iOS largeTitle pages (Dictionary, Styles) keep a 34pt left title. */
  @Prop largeTitle: boolean = false;
  @Prop actionLabel: string = '';
  @Prop actionSymbol: string = '';
  @Prop scrollable: boolean = true;
  @Prop gap: number = Space.lg;
  onAction: () => void = () => {
  };
  @BuilderParam content: () => void;
  @StorageLink('bp') bp: string = 'md';
  @StorageLink('shortWindow') shortWindow: boolean = false;

  build() {
    Column() {
      // iOS inline navigation bar: 44pt row, centred 17pt semibold title and a
      // 44pt trailing toolbar button. An optional subtitle renders as a
      // caption2 line rather than a second large title.
      if (this.largeTitle) {
        Row({ space: Space.sm }) {
          Column({ space: Space.xxs }) {
            Text(this.title)
              .fontSize(Type.pageTitle)
              .lineHeight(Leading.pageTitle)
              .fontWeight(FontWeight.Bold)
              .fontColor(Palette.text1())
            if (this.subtitle.length > 0) {
              Text(this.subtitle)
                .fontSize(Type.caption)
                .lineHeight(Leading.caption)
                .fontColor(Palette.text2())
            }
          }
          .alignItems(HorizontalAlign.Start)
          .layoutWeight(1)
          if (this.actionLabel.length > 0 || this.actionSymbol.length > 0) {
            Button({ type: ButtonType.Circle }) {
              SymbolGlyph(symbolOf(this.actionSymbol.length > 0 ? this.actionSymbol : 'add'))
                .fontSize(Type.navTitle)
                .fontColor([Palette.accent()])
            }
            .width(Size.touch)
            .height(Size.touch)
            .backgroundColor(Palette.card())
            .hoverEffect(HoverEffect.Highlight)
            .accessibilityText(this.actionLabel)
            .onClick(() => this.onAction())
          }
        }
        .width('100%')
        .padding({ top: Space.xs, bottom: Space.sm })
      }
      Stack({ alignContent: Alignment.Center }) {
        Column({ space: 2 }) {
          Text(this.title)
            .fontSize(Type.navTitle)
            .lineHeight(Leading.navTitle)
            .fontWeight(FontWeight.Medium)
            .fontColor(Palette.text1())
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
          if (this.subtitle.length > 0 && !this.shortWindow) {
            Text(this.subtitle)
              .fontSize(Type.caption2)
              .lineHeight(Leading.caption2)
              .fontColor(Palette.text2())
              .maxLines(1)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
          }
        }
        .width('100%')
        .alignItems(HorizontalAlign.Center)

        Row() {
          Blank()
          if (this.actionLabel.length > 0 || this.actionSymbol.length > 0) {
            Button({ type: ButtonType.Circle }) {
              if (this.actionSymbol.length > 0) {
                SymbolGlyph(symbolOf(this.actionSymbol))
                  .fontSize(Type.navTitle)
                  .fontColor([Palette.accent()])
              } else {
                Text(this.actionLabel)
                  .fontSize(Type.caption)
                  .fontColor(Palette.accent())
              }
            }
            .width(Size.touch)
            .height(Size.touch)
            .backgroundColor(Palette.control())
            .hoverEffect(HoverEffect.Highlight)
            .accessibilityText(this.actionLabel.length > 0 ? this.actionLabel : '操作')
            .onClick(() => this.onAction())
          }
        }
        .width('100%')
      }
      .width('100%')
      .visibility(this.largeTitle ? Visibility.None : Visibility.Visible)
      .constraintSize({ minHeight: this.subtitle.length > 0 && !this.shortWindow ? 60 : Size.touch })

      if (this.scrollable) {
        Scroll() {
          Column({ space: this.gap }) {
            this.content()
          }
          .width('100%')
          .padding({ top: Space.sm, bottom: Layout.dockClearance() })
        }
        .layoutWeight(1)
        .width('100%')
        .align(Alignment.Top)
        .scrollBar(BarState.Off)
        .edgeEffect(EdgeEffect.Spring)
      } else {
        Column({ space: this.gap }) {
          this.content()
        }
        .layoutWeight(1)
        .width('100%')
        .padding({ top: Space.xs, bottom: Space.xs })
      }
    }
    .width('100%')
    .height('100%')
    .padding({ left: Layout.gutter(this.bp), right: Layout.gutter(this.bp) })
  }
}

/**
 * iOS `PinnedSearchBar` (NativeSearchField.swift:71-79): a 48vp field pinned above
 * the scrolling list, page gutter 20, 8vp vertical padding and a 0.5 hairline.
 * Content scrolls underneath it, like the `.searchable` drawer in the video.
 */
@Component
export struct PinnedSearchBar {
  @Prop placeholder: string = '';
  @Prop value: string = '';
  @Prop showHairline: boolean = true;
  /** False when the parent page already applies the 20vp page gutter. */
  @Prop inset: boolean = true;
  onInput: (text: string) => void = (text: string) => {
  };
  @StorageLink('bp') bp: string = 'md';

  build() {
    Column() {
      Row({ space: Space.xs }) {
        SymbolGlyph(symbolOf('search'))
          .fontSize(Size.iconSmall)
          .fontColor([Palette.text3()])
        TextInput({ placeholder: this.placeholder, text: this.value })
          .fontSize(Type.body)
          .fontColor(Palette.text1())
          .placeholderColor(Palette.text3())
          .backgroundColor(Color.Transparent)
          .padding(0)
          .layoutWeight(1)
          .onChange((text: string) => this.onInput(text))
        if (this.value.length > 0) {
          Button({ type: ButtonType.Circle }) {
            SymbolGlyph(symbolOf('close'))
              .fontSize(Size.iconSmall)
              .fontColor([Palette.text3()])
          }
          .width(32)
          .height(32)
          .backgroundColor(Color.Transparent)
          .accessibilityText('清除搜索内容')
          .onClick(() => this.onInput(''))
        }
      }
      .width('100%')
      .height(48)
      .padding({ left: Space.sm, right: Space.sm })
      .backgroundColor(Palette.cardAlt())
      .borderRadius(Radius.control)
    }
    .width('100%')
    .padding({
      left: this.inset ? Layout.gutter(this.bp) : 0,
      right: this.inset ? Layout.gutter(this.bp) : 0,
      top: Space.xs,
      bottom: Space.xs
    })
    .backgroundColor(Palette.bg())
    .border(this.showHairline
      ? { width: { bottom: 0.5 }, color: Palette.line() }
      : { width: 0, color: Color.Transparent })
  }
}

/**
 * iOS `sourceFilters` capsule track (VocabularyView.swift:133-153): a horizontally
 * scrolling track (`secondarySystemBackground`) whose selected chip is a white
 * capsule with a soft shadow. Chips carry an optional tinted glyph.
 */
export interface CapsuleChip {
  label: string;
  symbol: string;
  tint: string;
}

@Component
export struct CapsuleTrack {
  @Prop chips: CapsuleChip[] = [];
  @Prop selected: number = 0;
  onSelect: (index: number) => void = (index: number) => {
  };

  private ink(tint: string): Resource {
    if (tint === 'teal') return Palette.success();
    if (tint === 'accent') return Palette.accent();
    return Palette.text2();
  }

  build() {
    Scroll() {
      Row({ space: Space.xxs }) {
        ForEach(this.chips, (chip: CapsuleChip, index: number) => {
          Button({ type: ButtonType.Normal }) {
            Row({ space: 6 }) {
              if (chip.symbol.length > 0) {
                SymbolGlyph(symbolOf(chip.symbol))
                  .fontSize(Size.iconSmall)
                  .fontColor([this.ink(chip.tint)])
              }
              Text(chip.label)
                .fontSize(Type.body)
                .lineHeight(Leading.body)
                .fontWeight(index === this.selected ? FontWeight.Medium : FontWeight.Regular)
                .fontColor(Palette.text1())
                .maxLines(1)
            }
            .padding({ left: Space.md, right: Space.md, top: Space.sm, bottom: Space.sm })
            .backgroundColor(index === this.selected ? Palette.card() : Color.Transparent)
            .borderRadius(Radius.pill)
            .shadow(index === this.selected
              ? { radius: 4, color: Palette.line(), offsetY: 1 }
              : undefined)
          }
          .padding(0)
          .backgroundColor(Color.Transparent)
          .hoverEffect(HoverEffect.Highlight)
          .accessibilityText(chip.label)
          .accessibilitySelected(index === this.selected)
          .onClick(() => this.onSelect(index))
        }, (chip: CapsuleChip) => 'chip-' + chip.label)
      }
      .padding(5)
    }
    .scrollable(ScrollDirection.Horizontal)
    .scrollBar(BarState.Off)
    .align(Alignment.Start)
    .width('100%')
    .backgroundColor(Palette.control())
    .borderRadius(Radius.pill)
  }
}

/** iOS `.foregroundStyle(AppDesign.accent)` plain text action ("Filter by date"). */
@Component
export struct AccentTextButton {
  @Prop label: string;
  @Prop usable: boolean = true;
  onTap: () => void = () => {
  };

  build() {
    Button({ type: ButtonType.Normal }) {
      Text(this.label)
        .fontSize(Type.label)
        .lineHeight(Leading.label)
        .fontWeight(FontWeight.Regular)
        .fontColor(Palette.accent())
    }
    .padding({ left: 0, right: 0, top: Space.xxs, bottom: Space.xxs })
    .backgroundColor(Color.Transparent)
    .hoverEffect(HoverEffect.Highlight)
    .enabled(this.usable)
    .accessibilityText(this.label)
    .onClick(() => this.onTap())
  }
}

/** Single toast entry so every page reports success/failure the same way. */
export function notify(message: string): void {
  try {
    promptAction.showToast({ message: message, duration: 2000 });
  } catch (error) {
    console.warn('[OpenLess Toast] ' + message);
  }
}

/** Labelled text input; `multiline` switches to a text area. */
@Component
export struct InputField {
  @Prop label: string;
  @Prop placeholder: string = '';
  @Prop value: string = '';
  @Prop hint: string = '';
  @Prop password: boolean = false;
  @Prop multiline: boolean = false;
  onInput: (value: string) => void = (value: string) => {
  };

  build() {
    Column({ space: Space.xs }) {
      Text(this.label)
        .fontSize(Type.label)
        .lineHeight(Leading.label)
        .fontWeight(FontWeight.Medium)
        .fontColor(Palette.text1())

      if (this.multiline) {
        TextArea({ placeholder: this.placeholder, text: this.value })
          .fontSize(Type.body)
          .fontColor(Palette.text1())
          .placeholderColor(Palette.text3())
          .backgroundColor(Palette.cardAlt())
          .borderRadius(Radius.control)
          .constraintSize({ minHeight: 120 })
          .padding(Space.sm)
          .width('100%')
          .onChange((text: string) => this.onInput(text))
      } else {
        TextInput({ placeholder: this.placeholder, text: this.value })
          .type(this.password ? InputType.Password : InputType.Normal)
          .fontSize(Type.body)
          .fontColor(Palette.text1())
          .placeholderColor(Palette.text3())
          .backgroundColor(Palette.cardAlt())
          .borderRadius(Radius.control)
          .height(44)
          .padding({ left: Space.sm, right: Space.sm })
          .width('100%')
          .onChange((text: string) => this.onInput(text))
      }

      if (this.hint.length > 0) {
        Text(this.hint)
          .fontSize(Type.caption)
          .lineHeight(Leading.caption)
          .fontColor(Palette.text3())
      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
  }
}

/** Title block used inside `bindSheet` bodies. */
@Component
export struct SheetTitle {
  @Prop title: string;
  @Prop hint: string = '';
  /** Trailing text action, e.g. the iOS sheet's 完成 / 保存. */
  @Prop actionLabel: string = '';
  onAction: () => void = () => {
  };

  build() {
    Row({ space: Space.sm }) {
      Column({ space: Space.xxs }) {
        Text(this.title)
          .fontSize(Type.section)
          .lineHeight(Leading.section)
          .fontWeight(FontWeight.Medium)
          .fontColor(Palette.text1())
          .width('100%')
        if (this.hint.length > 0) {
          Text(this.hint)
            .fontSize(Type.label)
            .lineHeight(Leading.label)
            .fontColor(Palette.text3())
            .width('100%')
        }
      }
      .alignItems(HorizontalAlign.Start)
      .layoutWeight(1)

      if (this.actionLabel.length > 0) {
        TextButton({
          label: this.actionLabel,
          usable: true,
          onTap: () => this.onAction()
        })
      }
    }
    .width('100%')
    .alignItems(VerticalAlign.Top)
    .padding({ bottom: Space.sm })
  }
}
