import { Leading, Palette, Space, Type } from '../../common/Theme';
import { Surface, TextButton } from '../Kit';

/**
 * iOS HomeView.swift:146-159, the result card:
 *
 *   HStack { Text("实时转写" / "识别结果").font(.subheadline.weight(.semibold)); Spacer(); copy button }
 *   Text(output).font(.body).lineSpacing(5).textSelection(.enabled)
 *
 * The page owns clipboard I/O and the 已复制 reset timer; this card only renders.
 */
@Component
export struct TranscriptCard {
  @Prop text: string = '';
  @Prop copied: boolean = false;
  /** While the pipeline is busy iOS titles the card 「实时转写」. */
  @Prop active: boolean = false;
  onCopy: () => void = () => {
  };

  build() {
    Surface({ inset: Space.md }) {
      Column({ space: Space.md }) {
        Row({ space: Space.sm }) {
          Text(this.active ? '实时转写' : '识别结果')
            .fontSize(Type.label)
            .lineHeight(Leading.label)
            .fontWeight(FontWeight.Medium)
            .fontColor(Palette.text1())
            .layoutWeight(1)
          TextButton({
            label: this.copied ? '已复制' : '复制',
            symbol: this.copied ? 'check' : 'copy',
            onTap: () => {
              this.onCopy();
            }
          })
        }
        .width('100%')
        .alignItems(VerticalAlign.Center)

        Text(this.text)
          .fontSize(Type.body)
          .lineHeight(Leading.body + 5)
          .fontColor(Palette.text1())
          .width('100%')
          .copyOption(CopyOptions.LocalDevice)
      }
      .width('100%')
    }
  }
}
