import { KeyCode } from '@kit.InputKit';
import { Leading, Palette, Radius, Size, Space, Type } from '../../common/Theme';
import { activateOnKey, Tag, TextButton } from '../Kit';
import { symbolOf } from '../Icons';

/**
 * Presentational rows and cards for 词典 / 纠错规则 / 风格.
 *
 * Ported 1:1 from `ios/App/VocabularyView.swift` and
 * `ios/App/StyleLibraryView.swift`: every component here receives props and
 * callbacks only, owns no request state and never calls `Api`.
 *
 * Layout numbers follow the iOS views:
 *   - vocabulary row: 24pt source slot, `.title2` phrase, `.caption` "已停用"
 *     tag, 18/24pt padding, divider indented past the icon (56pt);
 *   - style card: 36pt `IconTile`, `.headline` name, `.subheadline` summary
 *     (2–3 lines), `.subheadline` metadata, one trailing selection capsule,
 *     14pt padding inside the 18pt card radius.
 */

/** Corner radius of the little icon tile (iOS `IconTile(size: 36)` → 12). */
const TILE: number = 36;
/** Divider indent in front of the row content (iOS pads the divider by 56). */
const ROW_ICON_SLOT: number = 24;

/**
 * 词典行：`.title2` phrase first, matching `VocabularyView.vocabularyList`.
 * The leading slot carries the source glyph (`leaf.fill` / `sparkle` on iOS).
 */
@Component
export struct VocabularyRow {
  @Prop phrase: string = '';
  /** `learned` | `manual` — classified from the entry note, as iOS does. */
  @Prop source: string = 'manual';
  @Prop isEnabled: boolean = true;
  @Prop selected: boolean = false;
  @Prop divider: boolean = false;
  onSelect: () => void = () => {
  };

  private learned(): boolean {
    return this.source === 'learned';
  }

  private sourceSymbol(): string {
    return this.learned() ? 'sparkles' : 'vocab';
  }

  build() {
    Column() {
      Row({ space: Space.sm }) {
        Stack({ alignContent: Alignment.Center }) {
          SymbolGlyph(symbolOf(this.sourceSymbol()))
            .fontSize(Type.body)
            .fontColor([this.learned() ? Palette.accent() : Palette.text3()])
        }
        .width(ROW_ICON_SLOT)
        .height(ROW_ICON_SLOT)

        Text(this.phrase)
          .fontSize(Type.section)
          .lineHeight(Leading.section)
          .fontWeight(FontWeight.Medium)
          .fontColor(this.isEnabled ? Palette.text1() : Palette.text3())
          .maxLines(2)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .layoutWeight(1)

        if (!this.isEnabled) {
          Tag({ text: '已停用' })
        }
      }
      .width('100%')
      .padding({ left: Space.md, right: Space.md, top: Space.lg, bottom: Space.lg })
      .backgroundColor(this.selected ? Palette.accentSoft() : Palette.card())
      .hoverEffect(HoverEffect.Highlight)
      .focusable(true)
      .onKeyEvent((event: KeyEvent) => activateOnKey(event, () => this.onSelect()))
      .onClick(() => this.onSelect())

      if (this.divider) {
        Divider()
          .strokeWidth(0.5)
          .color(Palette.line())
          .margin({ left: Space.md + ROW_ICON_SLOT + Space.sm })
      }
    }
    .width('100%')
    .accessibilityGroup(true)
    .accessibilityText(this.phrase + '，' + (this.learned() ? '自动添加' : '手动添加') +
      (this.isEnabled ? '' : '，已停用'))
  }
}

/**
 * 纠错规则行：和词典行同一套排版（大字、18/24 内边距、56 分隔线缩进），
 * 只是把「错的样子 → 正确写法」摆成两行。
 */
@Component
export struct CorrectionRuleRow {
  @Prop pattern: string = '';
  @Prop replacement: string = '';
  /** `learned` | `manual` — mirrors the Core rule source marker. */
  @Prop source: string = 'manual';
  @Prop isEnabled: boolean = true;
  @Prop selected: boolean = false;
  @Prop divider: boolean = false;
  onSelect: () => void = () => {
  };

  private learned(): boolean {
    return this.source === 'learned';
  }

  build() {
    Column() {
      Row({ space: Space.sm }) {
        Stack({ alignContent: Alignment.Center }) {
          SymbolGlyph(symbolOf(this.learned() ? 'sparkles' : 'corrections'))
            .fontSize(Type.body)
            .fontColor([this.learned() ? Palette.accent() : Palette.text3()])
        }
        .width(ROW_ICON_SLOT)
        .height(ROW_ICON_SLOT)

        Column({ space: 2 }) {
          Text(this.pattern)
            .fontSize(Type.label)
            .lineHeight(Leading.label)
            .fontColor(Palette.text3())
            .decoration({ type: TextDecorationType.LineThrough, color: Palette.text3() })
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
            .width('100%')
          Text(this.replacement.length > 0 ? this.replacement : '（删除）')
            .fontSize(Type.section)
            .lineHeight(Leading.section)
            .fontWeight(FontWeight.Medium)
            .fontColor(this.isEnabled ? Palette.text1() : Palette.text3())
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
            .width('100%')
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)

        if (!this.isEnabled) {
          Tag({ text: '已停用' })
        }
      }
      .width('100%')
      .padding({ left: Space.md, right: Space.md, top: Space.lg, bottom: Space.lg })
      .backgroundColor(this.selected ? Palette.accentSoft() : Palette.card())
      .hoverEffect(HoverEffect.Highlight)
      .focusable(true)
      .onKeyEvent((event: KeyEvent) => activateOnKey(event, () => this.onSelect()))
      .onClick(() => this.onSelect())

      if (this.divider) {
        Divider()
          .strokeWidth(0.5)
          .color(Palette.line())
          .margin({ left: Space.md + ROW_ICON_SLOT + Space.sm })
      }
    }
    .width('100%')
    .accessibilityGroup(true)
    .accessibilityText(this.pattern + ' 改为 ' + this.replacement +
      (this.isEnabled ? '' : '，已停用'))
  }
}

/**
 * 风格卡：`StyleLibraryView.StyleCard` + `StyleSummary`.
 *
 * One card works for both sections — the market card simply passes 下载/点赞
 * metadata and a 安装/点赞 pair of actions, while a local card passes 使用 and
 * 导出. "使用中" and "已安装" are badges next to the name; the trailing capsule
 * is the single selection control of the iOS card.
 */
@Component
export struct StyleSummaryCard {
  @Prop name: string = '';
  @Prop summary: string = '';
  @Prop metadata: string = '';
  /** `使用中` / `已安装` / `内置` … rendered as a tag next to the name. */
  @Prop badge: string = '';
  @Prop badgeTone: string = 'neutral';
  /** Trailing capsule label. Empty hides the control (iOS 使用中 is disabled). */
  @Prop primaryLabel: string = '';
  @Prop primaryEnabled: boolean = true;
  /** Secondary text action under the card, e.g. 导出 / 点赞. */
  @Prop secondaryLabel: string = '';
  @Prop muted: boolean = false;
  @Prop selected: boolean = false;
  onOpen: () => void = () => {
  };
  onPrimary: () => void = () => {
  };
  onSecondary: () => void = () => {
  };

  private edgeColor(): Resource {
    return this.selected || this.badge === '使用中' ? Palette.accent() : Palette.line();
  }

  @Builder
  capsule() {
    Row() {
    Button({ type: ButtonType.Capsule }) {
      Row({ space: Space.xxs }) {
        if (this.selected) {
          SymbolGlyph(symbolOf('check'))
            .fontSize(Type.caption2)
            .fontColor([Palette.onAccent()])
        }
        Text(this.primaryLabel)
          .fontSize(Type.caption)
          .lineHeight(Leading.caption)
          .fontWeight(FontWeight.Medium)
          .fontColor(this.selected ? Palette.onAccent() : Palette.accent())
          .maxLines(1)
      }
    }
    .constraintSize({ minWidth: 56, minHeight: 32 })
    .height(32)
    .padding({ left: Space.sm, right: Space.sm })
    .backgroundColor(this.selected ? Palette.accent() : Color.Transparent)
    .borderRadius(Radius.pill)
    .border({ width: 1, color: Palette.accent() })
    .opacity(this.primaryEnabled ? 1 : 0.45)
    .enabled(this.primaryEnabled)
    .hoverEffect(HoverEffect.Highlight)
    .accessibilityText(this.primaryLabel)
    .onClick((event: ClickEvent) => {
      if (this.primaryEnabled) this.onPrimary();
    })
    .onKeyEvent((event: KeyEvent) => {
      // Keep native Button activation, but never activate the enclosing card too.
      if (event.keyCode === KeyCode.KEYCODE_ENTER || event.keyCode === KeyCode.KEYCODE_NUMPAD_ENTER ||
        event.keyCode === KeyCode.KEYCODE_SPACE) event.stopPropagation();
    })
    }
    // A disabled native Button otherwise falls through to the card (Huawei FAQ 243).
    // https://developer.huawei.com/consumer/cn/doc/doccenter-dev-faq/faqs-arkui-243
    .hitTestBehavior(HitTestMode.Block)
  }

  build() {
    Column({ space: Space.sm }) {
      Row({ space: Space.sm }) {
        Stack({ alignContent: Alignment.Center }) {
          SymbolGlyph(symbolOf('sparkles'))
            .fontSize(Size.icon)
            .fontColor([Palette.accent()])
        }
        .width(TILE)
        .height(TILE)
        .backgroundColor(Palette.accentSoft())
        .borderRadius(Radius.control)

        Column({ space: Space.xxs }) {
          Row({ space: Space.xs }) {
            Text(this.name)
              .fontSize(Type.cardTitle)
              .lineHeight(Leading.cardTitle)
              .fontWeight(FontWeight.Medium)
              .fontColor(Palette.text1())
              .maxLines(2)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
              .layoutWeight(1)
            if (this.badge.length > 0) {
              Tag({ text: this.badge, tone: this.badgeTone })
            }
            if (this.primaryLabel.length > 0) {
              this.capsule()
            }
          }
          .width('100%')
          .alignItems(VerticalAlign.Center)

          if (this.summary.length > 0) {
            Text(this.summary)
              .fontSize(Type.label)
              .lineHeight(Leading.label)
              .fontColor(Palette.text2())
              .maxLines(3)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
              .width('100%')
          }
          if (this.metadata.length > 0) {
            Text(this.metadata)
              .fontSize(Type.caption)
              .lineHeight(Leading.caption)
              .fontColor(Palette.text3())
              .maxLines(1)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
              .width('100%')
          }
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)
      }
      .width('100%')
      .alignItems(VerticalAlign.Top)

      if (this.secondaryLabel.length > 0) {
        Row({ space: Space.sm }) {
          TextButton({ label: this.secondaryLabel, tone: 'quiet', onTap: () => this.onSecondary() })
          Blank()
        }
        .width('100%')
        .alignItems(VerticalAlign.Center)
      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
    .padding(Space.md)
    .backgroundColor(Palette.card())
    .borderRadius(Radius.card)
    .border({ width: this.selected || this.badge === '使用中' ? 1.5 : 0.5, color: this.edgeColor() })
    .opacity(this.muted ? 0.55 : 1)
    .hoverEffect(HoverEffect.Highlight)
    .focusable(true)
    .onKeyEvent((event: KeyEvent) => activateOnKey(event, () => this.onOpen()))
    .onClick(() => this.onOpen())
    .accessibilityGroup(true)
    .accessibilityText(this.name + '，' + this.summary + '，' + this.badge)
  }
}
