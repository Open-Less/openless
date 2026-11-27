import { Motion, Palette, Radius, Type, Leading } from '../common/Theme';
import { symbolOf } from './Icons';

const BAR_PROFILE: number[] = [0.45, 0.7, 0.9, 1, 0.9, 0.7, 0.45];

/**
 * Voice control modelled on the iOS main screen (ios/PlatformSupport SpeakButton):
 *
 *   idle        a wide capsule with a microphone and 「开始说话」
 *   recording   grows into a filled circle with a seven-bar waveform and a soft
 *               level ring behind it
 *   processing  a compact capsule with an activity indicator and 「转写中…」
 *
 * Tap toggles start/stop; the recorder itself lives in the core, this control only
 * reflects `dictPhase` / `dictLevel` and reports intent.
 */
@Component
export struct VoiceControl {
  @Prop phase: string = 'idle';
  @Prop level: number = 0;
  @Prop usable: boolean = true;
  onToggle: () => void = () => {
  };

  private recording(): boolean {
    return this.phase === 'recording';
  }

  private busy(): boolean {
    return this.phase === 'processing' || this.phase === 'starting' || this.phase === 'cancelling';
  }

  private amplitude(): number {
    if (!this.recording()) return 0.1;
    const value: number = this.level;
    if (value < 0.05) return 0.05;
    return value > 1 ? 1 : value;
  }

  build() {
    Stack() {
      if (this.recording()) {
        Column()
          .width(150)
          .height(150)
          .borderRadius(Radius.pill)
          .backgroundColor(Palette.dangerSoft())
          .scale({ x: 1 + this.amplitude() * 0.12, y: 1 + this.amplitude() * 0.12 })
          .opacity(0.9)
          .animation({ duration: Motion.quick, curve: Curve.EaseOut })
      }

      Column() {
        if (this.recording()) {
          Row({ space: 4 }) {
            ForEach(BAR_PROFILE, (profile: number, index: number) => {
              Column()
                .width(4)
                .height(8 + this.amplitude() * 40 * profile)
                .borderRadius(2)
                .backgroundColor(Palette.onAccent())
                .animation({ duration: Motion.quick, curve: Curve.EaseOut })
            }, (profile: number, index: number) => 'bar-' + index.toString())
          }
          .height(52)
        } else if (this.busy()) {
          Row({ space: 8 }) {
            LoadingProgress()
              .width(20)
              .height(20)
              .color(Palette.onAccent())
            Text(this.phase === 'starting' ? '准备中…' : '转写中…')
              .fontSize(Type.label)
              .fontWeight(FontWeight.Medium)
              .fontColor(Palette.onAccent())
          }
        } else {
          Row({ space: 8 }) {
            SymbolGlyph(symbolOf('mic'))
              .fontSize(22)
              .fontColor([Palette.onAccent()])
            Text('开始说话')
              .fontSize(Type.body)
              .lineHeight(Leading.body)
              .fontWeight(FontWeight.Medium)
              .fontColor(Palette.onAccent())
          }
        }
      }
      .width(this.recording() ? 112 : 152)
      .height(this.recording() ? 112 : 60)
      .justifyContent(FlexAlign.Center)
      .backgroundColor(this.recording() ? Palette.danger() : Palette.accent())
      .borderRadius(Radius.pill)
      .opacity(this.usable ? 1 : 0.5)
      .animation({ duration: Motion.base, curve: Curve.EaseOut })
      .onClick(() => {
        if (this.usable) this.onToggle();
      })
      .stateStyles({
        normal: { .scale({ x: 1, y: 1 }) },
        pressed: { .scale({ x: 0.96, y: 0.96 }) }
      })
    }
    .width('100%')
    .height(176)
    .alignContent(Alignment.Center)
  }
}
