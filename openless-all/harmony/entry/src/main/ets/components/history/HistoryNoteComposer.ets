import { Leading, Palette, Space, Type } from '../../common/Theme';
import {
  ErrorNote,
  InputField,
  PrimaryButton,
  SectionTitle,
  Surface,
  Tag,
  TextButton,
  TonalButton
} from '../Kit';

/**
 * 速记 (quick note) — Harmony-only, kept behind the 记录/速记 segment but painted
 * with the same card vocabulary as the ported history screen: 18 radius cards,
 * 0.5 hairline, 16 inset, iOS type scale.
 *
 * Presentational only: the page owns the draft, its debounced persistence and the
 * dictation session; this component only renders and forwards events.
 */
const NOTE_TIPS: string =
  '草稿会自动保存在这台设备上，离开页面再回来可以接着写；点「清空」会连同保存的草稿一起删除。' +
    '语音追加复用听写链路，转写结果接在最后一段之后。';

@Component
export struct HistoryNoteComposer {
  @Prop text: string = '';
  @Prop hint: string = '';
  @Prop phase: string = 'idle';
  @Prop statusText: string = '';
  @Prop canDictate: boolean = true;
  onInput: (value: string) => void = (value: string) => {
  };
  onDictate: () => void = () => {
  };
  onCopy: () => void = () => {
  };
  onClear: () => void = () => {
  };

  build() {
    Column({ space: Space.md }) {
      if (this.statusText.length > 0) {
        ErrorNote({
          message: this.statusText,
          onRetry: () => {
            this.onDictate();
          }
        })
      }

      Surface({ inset: Space.md }) {
        Column({ space: Space.sm }) {
          SectionTitle({ title: '草稿', hint: this.hint })

          Flex({ wrap: FlexWrap.Wrap, justifyContent: FlexAlign.Start, alignItems: ItemAlign.Center }) {
            Row() {
              PrimaryButton({
                label: this.phase === 'recording' ? '结束并追加' : '语音追加',
                usable: this.canDictate,
                onTap: () => {
                  this.onDictate();
                }
              })
            }
            .margin({ right: Space.xs, bottom: Space.xs })

            Row() {
              TonalButton({
                label: '复制全部',
                symbol: 'copy',
                usable: this.text.length > 0,
                onTap: () => {
                  this.onCopy();
                }
              })
            }
            .margin({ right: Space.xs, bottom: Space.xs })

            Row() {
              TextButton({
                label: '清空',
                symbol: 'trash',
                tone: 'danger',
                usable: this.text.length > 0,
                onTap: () => {
                  this.onClear();
                }
              })
            }
            .margin({ right: Space.xs, bottom: Space.xs })
          }
          .width('100%')

          Row({ space: Space.xs }) {
            Tag({ text: this.text.length.toString() + ' 字', tone: 'neutral' })
            if (this.phase === 'recording') {
              Tag({ text: '正在录音', tone: 'danger' })
            } else if (this.phase === 'processing') {
              Tag({ text: '转写中', tone: 'accent' })
            }
          }
          .width('100%')
        }
        .width('100%')
        .alignItems(HorizontalAlign.Start)
      }

      Surface({ inset: Space.md }) {
        InputField({
          label: '内容',
          placeholder: '在这里写，或用上面的语音追加一段',
          value: this.text,
          multiline: true,
          onInput: (value: string) => {
            this.onInput(value);
          }
        })
      }

      Surface({ tone: 'alt', inset: Space.md }) {
        Column({ space: Space.xs }) {
          SectionTitle({ title: '说明' })
          Text(NOTE_TIPS)
            .fontSize(Type.label)
            .lineHeight(Leading.label)
            .fontColor(Palette.text2())
            .width('100%')
        }
        .width('100%')
        .alignItems(HorizontalAlign.Start)
      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
  }
}
