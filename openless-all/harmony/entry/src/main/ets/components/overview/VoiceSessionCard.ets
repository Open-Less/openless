import { Leading, Palette, Radius, Size, Space, Type } from '../../common/Theme';
import { symbolOf } from '../Icons';
import { Surface, TextButton } from '../Kit';
import { VoiceControl } from '../VoiceControl';

/**
 * iOS HomeView.swift:38-114 (`voiceCard`), one card, in the same order:
 *
 *   HStack  7pt status dot + subheadline state + trailing timer / language label
 *   ZStack  the microphone control
 *   Text    footnote hint
 *   Button  destructive 「取消」 while the phase is busy
 *
 * `.productCard(padding: 16)` is the shared `Surface` card (18vp radius,
 * hairline outline, card fill). One deliberate Harmony state: until a speech key
 * exists the iOS screen shows its 「启用语音」 capsule instead of the mic control,
 * and the Harmony equivalent is the same capsule opening the credentials page.
 *
 * Presentation only: recording, timer and errors stay in `OverviewPage`.
 */
@Component
export struct VoiceSessionCard {
  @Prop phase: string = 'idle';
  @Prop level: number = 0;
  /** A start/stop request is in flight. */
  @Prop busy: boolean = false;
  /** The recorder or engine is busy (starting / recording / processing / cancelling). */
  @Prop active: boolean = false;
  @Prop title: string = '';
  @Prop tone: Resource = Palette.accent();
  @Prop clock: string = '00:00';
  @Prop hint: string = '';
  /** A transcription key exists; otherwise the card offers to open credentials. */
  @Prop configured: boolean = true;
  onToggle: () => void = () => {
  };
  onCancel: () => void = () => {
  };
  onSetup: () => void = () => {
  };

  build() {
    Surface({ inset: Space.md }) {
      Column({ space: Space.md }) {
        Row({ space: Space.xs }) {
          Column()
            .width(7)
            .height(7)
            .borderRadius(Radius.pill)
            .backgroundColor(this.tone)
          Text(this.title)
            .fontSize(Type.label)
            .lineHeight(Leading.label)
            .fontWeight(FontWeight.Medium)
            .fontColor(Palette.text1())
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
            .layoutWeight(1)
          if (this.active) {
            Text(this.clock)
              .fontSize(Type.label)
              .lineHeight(Leading.label)
              .fontColor(Palette.text2())
          } else {
            Text('中英混合自动识别')
              .fontSize(Type.label)
              .lineHeight(Leading.label)
              .fontColor(Palette.text2())
              .maxLines(1)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
          }
        }
        .width('100%')
        .constraintSize({ minHeight: Size.touch })
        .alignItems(VerticalAlign.Center)

        if (this.configured) {
          VoiceControl({
            phase: this.phase,
            level: this.level,
            usable: !this.busy,
            onToggle: () => {
              this.onToggle();
            }
          })
        } else {
          Row() {
            // iOS HomeView.swift:68-82 — `.foregroundStyle(systemBackground)` on a
            // `.primary` capsule: black in light, white in dark, min 157x60.
            Button({ type: ButtonType.Capsule }) {
              Row({ space: 9 }) {
                SymbolGlyph(symbolOf('mic'))
                  .fontSize(21)
                  .fontColor([Palette.bg()])
                Text('启用语音')
                  .fontSize(Type.caption)
                  .lineHeight(Leading.caption)
                  .fontWeight(FontWeight.Medium)
                  .fontColor(Palette.bg())
              }
              .padding({ left: 22, right: 22, top: 17, bottom: 17 })
            }
            .constraintSize({ minWidth: 157, minHeight: 60 })
            .backgroundColor(Palette.text1())
            .enabled(!this.busy)
            .opacity(this.busy ? 0.4 : 1)
            .hoverEffect(HoverEffect.Highlight)
            .accessibilityText('启用语音')
            .onClick(() => {
              this.onSetup();
            })
          }
          .width('100%')
          .height(76)
          .justifyContent(FlexAlign.Center)
        }

        if (this.hint.length > 0) {
          Text(this.hint)
            .fontSize(Type.caption)
            .lineHeight(Leading.caption)
            .fontColor(Palette.text2())
            .textAlign(TextAlign.Center)
            .width('100%')
        }

        if (this.active) {
          Row() {
            TextButton({
              label: '取消',
              tone: 'danger',
              onTap: () => {
                this.onCancel();
              }
            })
          }
          .width('100%')
          .justifyContent(FlexAlign.Center)
        }
      }
      .width('100%')
    }
  }
}
