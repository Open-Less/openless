import { C, R, S, T, cardShadow } from '../common/Tokens';
import { symbolOf } from './Icons';

/**
 * Shared surfaces for the native pages.
 *
 * HarmonyOS pages are built from cards on a tinted background with a labelled
 * section header; every component here is the same shape at every breakpoint, so a
 * phone column and a 2-in-1 multi-column layout stay consistent.
 */
@Component
export struct SectionHeader {
  @Prop title: string;
  @Prop hint: string = '';

  build() {
    Column({ space: S.xs }) {
      Text(this.title)
        .fontSize(T.headline)
        .fontWeight(FontWeight.Medium)
        .fontColor(C.textPrimary())
      if (this.hint.length > 0) {
        Text(this.hint)
          .fontSize(T.footnote)
          .fontColor(C.textTertiary())
      }
    }
    .alignItems(HorizontalAlign.Start)
    .width('100%')
  }
}

@Component
export struct Card {
  @Prop inset: number = S.lg;
  @BuilderParam content: () => void;

  build() {
    Column() {
      this.content()
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
    .padding(this.inset)
    .backgroundColor(C.glass())
    .backgroundBlurStyle(BlurStyle.COMPONENT_THICK)
    .borderRadius(R.xl)
    .border({ width: 0.5, color: C.border() })
    .shadow(cardShadow())
  }
}

@Component
export struct EmptyState {
  @Prop symbol: string = 'sparkles';
  @Prop title: string;
  @Prop hint: string = '';

  build() {
    Column({ space: S.sm }) {
      SymbolGlyph(symbolOf(this.symbol))
        .fontSize(40)
        .fontColor([C.textTertiary()])
        .margin({ bottom: S.xs })
      Text(this.title)
        .fontSize(T.body)
        .fontWeight(FontWeight.Medium)
        .fontColor(C.textSecondary())
      if (this.hint.length > 0) {
        Text(this.hint)
          .fontSize(T.footnote)
          .fontColor(C.textTertiary())
          .textAlign(TextAlign.Center)
          .padding({ left: S.xl, right: S.xl })
      }
    }
    .width('100%')
    .padding({ top: S.xxxl, bottom: S.xxxl })
    .justifyContent(FlexAlign.Center)
  }
}

@Component
export struct Pill {
  @Prop text: string;
  @Prop tone: string = 'neutral';

  private tint(): Resource {
    if (this.tone === 'accent') return C.accentSoft();
    if (this.tone === 'success') return C.successSoft();
    if (this.tone === 'warning') return C.warningSoft();
    if (this.tone === 'danger') return C.dangerSoft();
    return C.sunken();
  }

  private fgColor(): Resource {
    if (this.tone === 'accent') return C.accent();
    if (this.tone === 'success') return C.success();
    if (this.tone === 'warning') return C.warning();
    if (this.tone === 'danger') return C.danger();
    return C.textSecondary();
  }

  build() {
    Text(this.text)
      .fontSize(T.caption)
      .fontWeight(FontWeight.Medium)
      .fontColor(this.fgColor())
      .padding({ left: S.sm, right: S.sm, top: S.xxs, bottom: S.xxs })
      .backgroundColor(this.tint())
      .borderRadius(R.pill)
  }
}

@Component
export struct PrimaryButton {
  @Prop label: string;
  @Prop usable: boolean = true;
  @Prop destructive: boolean = false;
  onTap: () => void = () => {
  };

  build() {
    Button(this.label)
      .fontSize(T.body)
      .fontWeight(FontWeight.Medium)
      .fontColor(this.destructive ? C.textOnAccent() : C.textOnAccent())
      .backgroundColor(this.destructive ? C.danger() : C.accent())
      .opacity(this.usable ? 1 : 0.4)
      .height(44)
      .borderRadius(R.pill)
      .padding({ left: S.xl, right: S.xl })
      .enabled(this.usable)
      .onClick(() => {
        if (this.usable) this.onTap();
      })
  }
}

@Component
export struct GhostButton {
  @Prop label: string;
  @Prop symbol: string = '';
  @Prop usable: boolean = true;
  onTap: () => void = () => {
  };

  build() {
    Row({ space: S.xs }) {
      if (this.symbol.length > 0) {
        SymbolGlyph(symbolOf(this.symbol))
          .fontSize(16)
          .fontColor([C.accent()])
      }
      Text(this.label)
        .fontSize(T.callout)
        .fontColor(C.accent())
    }
    .padding({ left: S.md, right: S.md, top: S.sm, bottom: S.sm })
    .backgroundColor(C.accentSoft())
    .borderRadius(R.pill)
    .opacity(this.usable ? 1 : 0.4)
    .onClick(() => {
      if (this.usable) this.onTap();
    })
  }
}

/** One settings-style row: label + optional value + chevron/toggle slot. */
@Component
export struct InfoRow {
  @Prop label: string;
  @Prop value: string = '';
  @Prop hint: string = '';
  @Prop showChevron: boolean = false;
  @Prop showDivider: boolean = true;
  onTap: () => void = () => {
  };

  build() {
    Column() {
      Row({ space: S.md }) {
        Column({ space: S.xxs }) {
          Text(this.label)
            .fontSize(T.body)
            .fontColor(C.textPrimary())
          if (this.hint.length > 0) {
            Text(this.hint)
              .fontSize(T.caption)
              .fontColor(C.textTertiary())
          }
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)

        if (this.value.length > 0) {
          Text(this.value)
            .fontSize(T.callout)
            .fontColor(C.textSecondary())
            .textAlign(TextAlign.End)
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
            .constraintSize({ maxWidth: 180 })
        }
        if (this.showChevron) {
          SymbolGlyph(symbolOf('chevron'))
            .fontSize(14)
            .fontColor([C.textTertiary()])
        }
      }
      .width('100%')
      .padding({ top: S.md, bottom: S.md })
      .onClick(() => this.onTap())

      if (this.showDivider) {
        Divider().color(C.divider()).strokeWidth(0.5)
      }
    }
    .width('100%')
  }
}

@Component
export struct ToggleRow {
  @Prop label: string;
  @Prop hint: string = '';
  @Prop checked: boolean = false;
  @Prop showDivider: boolean = true;
  onToggle: (value: boolean) => void = (value: boolean) => {
  };

  build() {
    Column() {
      Row({ space: S.md }) {
        Column({ space: S.xxs }) {
          Text(this.label)
            .fontSize(T.body)
            .fontColor(C.textPrimary())
          if (this.hint.length > 0) {
            Text(this.hint)
              .fontSize(T.caption)
              .fontColor(C.textTertiary())
          }
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)

        Toggle({ type: ToggleType.Switch, isOn: this.checked })
          .selectedColor(C.accent())
          .onChange((value: boolean) => this.onToggle(value))
      }
      .width('100%')
      .padding({ top: S.sm, bottom: S.sm })

      if (this.showDivider) {
        Divider().color(C.divider()).strokeWidth(0.5)
      }
    }
    .width('100%')
  }
}

@Component
export struct SearchField {
  @Prop placeholder: string = '搜索';
  @Prop value: string = '';
  onInput: (value: string) => void = (value: string) => {
  };

  build() {
    Row({ space: S.sm }) {
      SymbolGlyph(symbolOf('search'))
        .fontSize(16)
        .fontColor([C.textTertiary()])
      TextInput({ placeholder: this.placeholder, text: this.value })
        .fontSize(T.body)
        .backgroundColor(Color.Transparent)
        .placeholderColor(C.textTertiary())
        .padding(0)
        .layoutWeight(1)
        .onChange((text: string) => this.onInput(text))
      if (this.value.length > 0) {
        SymbolGlyph(symbolOf('close'))
          .fontSize(16)
          .fontColor([C.textTertiary()])
          .onClick(() => this.onInput(''))
      }
    }
    .width('100%')
    .height(44)
    .padding({ left: S.md, right: S.md })
    .backgroundColor(C.sunken())
    .borderRadius(R.md)
  }
}

/** Compact metric tile used by the overview header. */
@Component
export struct MetricTile {
  @Prop label: string;
  @Prop value: string;
  @Prop unit: string = '';

  build() {
    Column({ space: S.xxs }) {
      Row({ space: S.xxs }) {
        Text(this.value)
          .fontSize(T.title)
          .fontWeight(FontWeight.Medium)
          .fontColor(C.textPrimary())
        if (this.unit.length > 0) {
          Text(this.unit)
            .fontSize(T.caption)
            .fontColor(C.textTertiary())
        }
      }
      .alignItems(VerticalAlign.Bottom)

      Text(this.label)
        .fontSize(T.caption)
        .fontColor(C.textSecondary())
    }
    .alignItems(HorizontalAlign.Start)
    .layoutWeight(1)
  }
}

/**
 * Label / value row used by the summary cards.
 *
 * A `@Builder` with string parameters is evaluated once: when the value arrives
 * after an async load, the row keeps rendering the first (empty) string. As a
 * component with `@Prop`, it re-renders when the value changes.
 */
@Component
export struct StatusRow {
  @Prop label: string;
  @Prop value: string;
  @Prop showChevron: boolean = true;
  @Prop showDivider: boolean = true;
  onTap: () => void = () => {
  };

  build() {
    Column() {
      Row({ space: S.sm }) {
        Text(this.label)
          .fontSize(T.callout)
          .fontColor(C.textSecondary())
          .layoutWeight(1)
        Text(this.value.length > 0 ? this.value : '—')
          .fontSize(T.callout)
          .fontColor(C.textPrimary())
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .constraintSize({ maxWidth: 220 })
        if (this.showChevron) {
          SymbolGlyph(symbolOf('chevron'))
            .fontSize(12)
            .fontColor([C.textTertiary()])
        }
      }
      .width('100%')
      .padding({ top: S.sm, bottom: S.sm })
      .onClick(() => this.onTap())

      if (this.showDivider) {
        Divider().color(C.divider()).strokeWidth(0.5)
      }
    }
    .width('100%')
  }
}

/** Page title block; every page starts with it so headers line up across layouts. */
@Component
export struct PageTitle {
  @Prop title: string;
  @Prop subtitle: string = '';

  build() {
    Column({ space: S.xxs }) {
      Text(this.title)
        .fontSize(T.display)
        .fontWeight(FontWeight.Medium)
        .fontColor(C.textPrimary())
      if (this.subtitle.length > 0) {
        Text(this.subtitle)
          .fontSize(T.footnote)
          .fontColor(C.textTertiary())
      }
    }
    .alignItems(HorizontalAlign.Start)
    .width('100%')
    .padding({ top: S.lg, bottom: S.md })
  }
}
