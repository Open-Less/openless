import { Leading, Palette, Radius, Size, Space, Type } from '../../common/Theme';
import { activateOnKey, Group, TextButton } from '../Kit';
import { symbolOf } from '../Icons';

/**
 * iOS `HistoryView.dateFilterRow` (HistoryView.swift lines 101-116), ported as the
 * same single line:
 *
 *   HStack  [按日期筛选] (button style Toggle) ........ 重置 (only when on)
 *   (when on)  DatePicker 开始 / DatePicker 结束, 8pt apart
 *
 * The page owns the dates; the pickers themselves are opened by the page so this
 * component keeps no business state.
 */
const TOGGLE_HEIGHT: number = 36;
const DATE_ROW_HEIGHT: number = 44;

@Component
struct HistoryDateRow {
  @Prop label: string = '';
  @Prop value: string = '';
  onTap: () => void = () => {
  };

  build() {
    Row({ space: Space.sm }) {
      Text(this.label)
        .fontSize(Type.label)
        .lineHeight(Leading.label)
        .fontColor(Palette.text1())
      Blank()
      Text(this.value)
        .fontSize(Type.label)
        .lineHeight(Leading.label)
        .fontColor(Palette.text2())
        .maxLines(1)
      SymbolGlyph(symbolOf('chevron'))
        .fontSize(Size.iconSmall)
        .fontColor([Palette.text3()])
    }
    .width('100%')
    .height(DATE_ROW_HEIGHT)
    .padding({ left: Space.md, right: Space.md })
    .hoverEffect(HoverEffect.Highlight)
    .focusable(true)
    .onKeyEvent((event: KeyEvent) => activateOnKey(event, () => {
      this.onTap();
    }))
    .onClick(() => {
      this.onTap();
    })
    .accessibilityText(this.label + ' ' + this.value)
  }
}

@Component
export struct HistoryDateFilter {
  @Prop filterOn: boolean = false;
  @Prop startLabel: string = '';
  @Prop endLabel: string = '';
  onToggle: () => void = () => {
  };
  onReset: () => void = () => {
  };
  onPickStart: () => void = () => {
  };
  onPickEnd: () => void = () => {
  };

  build() {
    Column({ space: Space.xs }) {
      Row({ space: Space.xs }) {
        Button({ type: ButtonType.Normal }) {
          Text(this.filterOn ? '按日期筛选 · 已开启' : '按日期筛选')
            .fontSize(Type.label)
            .lineHeight(Leading.label)
            .fontColor(Palette.accent())
        }
        .padding({ left: 0, right: 0, top: Space.xxs, bottom: Space.xxs })
        .backgroundColor(Color.Transparent)
        .hoverEffect(HoverEffect.Highlight)
        .accessibilityText('按日期筛选')
        .accessibilitySelected(this.filterOn)
        .focusable(true)
        .onKeyEvent((event: KeyEvent) => activateOnKey(event, () => {
          this.onToggle();
        }))
        .onClick(() => {
          this.onToggle();
        })

        Blank()

        if (this.filterOn) {
          TextButton({
            label: '重置',
            onTap: () => {
              this.onReset();
            }
          })
        }
      }
      .width('100%')
      .alignItems(VerticalAlign.Center)

      if (this.filterOn) {
        Group() {
          Column() {
            HistoryDateRow({
              label: '开始',
              value: this.startLabel,
              onTap: () => {
                this.onPickStart();
              }
            })
            Divider()
              .color(Palette.line())
              .strokeWidth(0.5)
              .margin({ left: Space.md })
            HistoryDateRow({
              label: '结束',
              value: this.endLabel,
              onTap: () => {
                this.onPickEnd();
              }
            })
          }
          .width('100%')
        }
      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
  }
}
