import { Layout, Leading, Palette, Radius, Space, Type } from '../../common/Theme';
import { SectionTitle, Surface, Tag, TextButton, TonalButton } from '../Kit';
import { HistoryTextCard } from './HistoryTextCard';

/**
 * The one detail surface used by both presentations, ported from
 * `HistoryDetailView` (HistoryView.swift lines 138-244):
 *
 *   status (subheadline semibold) / date (caption) / duration (caption)
 *   处理结果 card   → 识别原文 card          (text cards with 复制)
 *   录音 card       (play / stop, plus the Harmony-only 重新转写)
 *   重新润色        (style choice, hint when no text model is configured)
 *   重新润色 · <style> card
 *   删除记录        (destructive, 44pt, second confirmation owned by the page)
 *
 * Presentational only: the page owns the entry, the player, the style list and
 * the confirmation sheets.
 */

/** iOS `VStack(spacing: 20)` between detail sections. */
const DETAIL_GAP: number = 20;
/** iOS header `VStack(spacing: 8)`. */
const HEADER_GAP: number = 8;
/** Spacing inside a product card (iOS secondary VStacks). */
const CARD_GAP: number = 12;
const FAILURE_GAP: number = 6;

@Component
export struct HistoryDetail {
  @Prop status: string = '';
  @Prop date: string = '';
  @Prop duration: string = '';
  @Prop finalText: string = '';
  @Prop originalText: string = '';
  @Prop hasAudio: boolean = false;
  @Prop playing: boolean = false;
  @Prop busy: boolean = false;
  @Prop canPolish: boolean = false;
  @Prop showRepolish: boolean = false;
  @Prop styleLabel: string = '';
  @Prop repolished: string = '';
  @Prop failure: string = '';
  onPlay: () => void = () => {
  };
  onCopyFinal: () => void = () => {
  };
  onCopyOriginal: () => void = () => {
  };
  onCopyRepolished: () => void = () => {
  };
  onRetranscribe: () => void = () => {
  };
  onPickStyle: () => void = () => {
  };
  onDelete: () => void = () => {
  };

  build() {
    Scroll() {
      Column({ space: DETAIL_GAP }) {
        Column({ space: HEADER_GAP }) {
          Text(this.status)
            .fontSize(Type.label)
            .lineHeight(Leading.label)
            .fontWeight(FontWeight.Medium)
            .fontColor(Palette.text1())
            .width('100%')
            .textAlign(TextAlign.Start)
          Text(this.date)
            .fontSize(Type.caption)
            .lineHeight(Leading.caption)
            .fontColor(Palette.text2())
            .width('100%')
            .textAlign(TextAlign.Start)
          if (this.duration.length > 0) {
            Text(this.duration)
              .fontSize(Type.caption)
              .lineHeight(Leading.caption)
              .fontColor(Palette.text2())
              .width('100%')
              .textAlign(TextAlign.Start)
          }
        }
        .width('100%')
        .alignItems(HorizontalAlign.Start)

        if (this.finalText.length > 0) {
          HistoryTextCard({
            title: '处理结果',
            text: this.finalText,
            onCopy: () => {
              this.onCopyFinal();
            }
          })
        }
        if (this.originalText.length > 0) {
          HistoryTextCard({
            title: '识别原文',
            text: this.originalText,
            onCopy: () => {
              this.onCopyOriginal();
            }
          })
        }
        if (this.finalText.length === 0 && this.originalText.length === 0) {
          Text('尚未生成文字。有实际保存的录音时，可以在下方播放。')
            .fontSize(Type.label)
            .lineHeight(Leading.label)
            .fontColor(Palette.text2())
            .width('100%')
            .textAlign(TextAlign.Start)
        }

        if (this.hasAudio) {
          this.audioCard()
        }

        if (this.showRepolish) {
          this.repolishCard()
        }

        if (this.repolished.length > 0) {
          HistoryTextCard({
            title: this.styleLabel.length > 0 ? '重新润色 · ' + this.styleLabel : '重新润色',
            text: this.repolished,
            onCopy: () => {
              this.onCopyRepolished();
            }
          })
        }

        TextButton({
          label: '删除记录',
          symbol: 'trash',
          tone: 'danger',
          usable: !this.busy,
          onTap: () => {
            this.onDelete();
          }
        })

        if (this.failure.length > 0) {
          Row({ space: FAILURE_GAP }) {
            Stack() {
              Text('!')
                .fontSize(Type.caption2)
                .fontWeight(FontWeight.Bold)
                .fontColor(Palette.danger())
            }
            .width(15)
            .height(15)
            .borderRadius(Radius.pill)
            .border({ width: 1, color: Palette.danger() })
            Text('操作未完成，请稍后重试。')
              .fontSize(Type.caption)
              .lineHeight(Leading.caption)
              .fontColor(Palette.text2())
              .layoutWeight(1)
              .textAlign(TextAlign.Start)
          }
          .width('100%')
          .alignItems(VerticalAlign.Center)
        }
      }
      .width('100%')
      .alignItems(HorizontalAlign.Start)
      .padding({ bottom: Layout.dockClearance() })
    }
    .width('100%')
    .layoutWeight(1)
    .align(Alignment.Top)
    .scrollBar(BarState.Off)
    .edgeEffect(EdgeEffect.Spring)
  }

  /** iOS 录音 card: subheadline semibold title, play/stop, playback failure note. */
  @Builder
  audioCard() {
    Surface({ inset: Space.md }) {
      Column({ space: CARD_GAP }) {
        SectionTitle({ title: '录音' })
        Flex({ wrap: FlexWrap.Wrap, justifyContent: FlexAlign.Start, alignItems: ItemAlign.Center }) {
          Row() {
            TonalButton({
              label: this.playing ? '停止播放' : '播放录音',
              symbol: this.playing ? 'stop' : 'play',
              usable: !this.busy,
              onTap: () => {
                this.onPlay();
              }
            })
          }
          .margin({ right: Space.xs, bottom: Space.xs })

          Row() {
            TonalButton({
              label: this.busy ? '重新转写中…' : '重新转写',
              symbol: 'refresh',
              usable: !this.busy,
              onTap: () => {
                this.onRetranscribe();
              }
            })
          }
          .margin({ right: Space.xs, bottom: Space.xs })
        }
        .width('100%')
      }
      .width('100%')
      .alignItems(HorizontalAlign.Start)
    }
  }

  /** iOS 重新润色 card: style menu of the configured packs, disabled without a text model. */
  @Builder
  repolishCard() {
    Surface({ inset: Space.md }) {
      Column({ space: CARD_GAP }) {
        SectionTitle({
          title: '重新润色',
          hint: this.canPolish ? '选一种风格，结果会显示在下方' : '添加文字模型后可重新润色。'
        })
        Flex({ wrap: FlexWrap.Wrap, justifyContent: FlexAlign.Start, alignItems: ItemAlign.Center }) {
          Row() {
            TonalButton({
              label: this.busy ? '正在重新润色…' : '重新润色',
              symbol: 'sparkles',
              usable: this.canPolish && !this.busy,
              onTap: () => {
                this.onPickStyle();
              }
            })
          }
          .margin({ right: Space.xs, bottom: Space.xs })

          if (this.styleLabel.length > 0) {
            Row() {
              Tag({ text: this.styleLabel, tone: 'accent' })
            }
            .margin({ right: Space.xs, bottom: Space.xs })
          }
        }
        .width('100%')
      }
      .width('100%')
      .alignItems(HorizontalAlign.Start)
    }
  }
}
