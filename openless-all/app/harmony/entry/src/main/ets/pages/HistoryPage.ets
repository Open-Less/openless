import { Breakpoints } from '../common/Breakpoint';
import { AudioPlayback } from '../common/AudioPlayback';
import { Fmt } from '../common/Format';
import { Store } from '../common/Store';
import { C, R, S, T } from '../common/Tokens';
import { Card, EmptyState, GhostButton, Pill, SearchField, SectionHeader, StatusRow } from '../components/Common';
import { symbolOf } from '../components/Icons';
import { Api } from '../data/Api';
import { common } from '@kit.AbilityKit';
import { HistoryEntry, SettingsSnapshot, UserPreferences } from '../data/Types';

interface ModeLabel {
  id: string;
  label: string;
}

const MODE_LABELS: ModeLabel[] = [
  { id: 'raw', label: '原文' },
  { id: 'polish', label: '润色' },
  { id: 'translate', label: '翻译' },
  { id: 'structured', label: '结构化' }
];

function modeLabel(mode: string | undefined): string {
  if (mode === undefined) return '';
  for (let index = 0; index < MODE_LABELS.length; index++) {
    if (MODE_LABELS[index].id === mode) return MODE_LABELS[index].label;
  }
  return mode;
}

/**
 * History: a searchable list that becomes list + detail on a wide window.
 *
 * The detail pane is driven by `detailId` in AppStorage, so the same screen covers
 * a tablet split view, a 2-in-1 two-pane window and the phone push flow (where the
 * system back gesture pops the detail first).
 */
@Component
export struct HistoryPage {
  @StorageLink('bp') bp: string = 'md';
  @StorageLink('detailId') detailId: string = '';
  @StorageLink('backendRevision') revision: number = 0;

  @State private entries: HistoryEntry[] = [];
  @State private query: string = '';
  @State private busy: boolean = false;
  @State private error: string = '';
  @State private prefs: UserPreferences = {};
  @State private playing: boolean = false;
  private audio: AudioPlayback = new AudioPlayback();

  aboutToAppear(): void {
    this.reload();
  }

  private async reload(): Promise<void> {
    try {
      this.entries = await Api.history();
      const settings: SettingsSnapshot = await Api.settings();
      this.prefs = settings.preferences;
      this.error = '';
    } catch (error) {
      this.error = (error as Error).message;
    }
  }

  private filtered(): HistoryEntry[] {
    if (this.query.length === 0) return this.entries;
    const needle: string = this.query.toLowerCase();
    const result: HistoryEntry[] = [];
    for (let index = 0; index < this.entries.length; index++) {
      const entry: HistoryEntry = this.entries[index];
      const text: string = (entry.finalText + ' ' + (entry.rawTranscript ?? '') + ' ' + (entry.appName ?? '')).toLowerCase();
      if (text.indexOf(needle) >= 0) result.push(entry);
    }
    return result;
  }

  private selected(): HistoryEntry | undefined {
    for (let index = 0; index < this.entries.length; index++) {
      if (this.entries[index].id === this.detailId) return this.entries[index];
    }
    return undefined;
  }

  private async remove(id: string): Promise<void> {
    this.busy = true;
    try {
      await Api.deleteHistoryEntry(id);
      if (this.detailId === id) Store.put<string>('detailId', '');
      await this.reload();
    } catch (error) {
      this.error = (error as Error).message;
    } finally {
      this.busy = false;
    }
  }

  private async togglePlayback(entry: HistoryEntry): Promise<void> {
    if (this.playing) {
      await this.audio.stop();
      this.playing = false;
      return;
    }
    this.error = '';
    try {
      const payload = await Api.readRecording(entry.id);
      const context = this.getUIContext().getHostContext() as common.UIAbilityContext;
      await this.audio.play(context.cacheDir, entry.id, payload.base64, (playing: boolean) => {
        this.playing = playing;
      });
    } catch (error) {
      this.playing = false;
      this.error = '播放失败：' + (error as Error).message;
    }
  }

  private async repolish(entry: HistoryEntry): Promise<void> {
    this.busy = true;
    this.error = '';
    try {
      const source: string = entry.rawTranscript !== undefined && entry.rawTranscript.length > 0
        ? entry.rawTranscript : entry.finalText;
      const text: string = await Api.repolish(source, this.prefs.activeStylePackId ?? '');
      await Api.copyText(text);
      await this.reload();
    } catch (error) {
      this.error = (error as Error).message;
    } finally {
      this.busy = false;
    }
  }

  private async retranscribe(id: string): Promise<void> {
    this.busy = true;
    this.error = '';
    try {
      await Api.retranscribe(id);
      await this.reload();
    } catch (error) {
      this.error = (error as Error).message;
    } finally {
      this.busy = false;
    }
  }

  build() {
    Column() {
      Row({ space: S.md }) {
        Column({ space: S.xxs }) {
          Text('历史')
            .fontSize(T.display)
            .fontWeight(FontWeight.Medium)
            .fontColor(C.textPrimary())
          Text(this.entries.length.toString() + ' 条记录 · 全部保存在本机')
            .fontSize(T.footnote)
            .fontColor(C.textTertiary())
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)

        GhostButton({
          label: '刷新',
          symbol: 'refresh',
          onTap: () => {
            this.reload();
          }
        })
      }
      .width('100%')
      .padding({ top: S.lg, bottom: S.md })

      SearchField({
        placeholder: '搜索转写内容或应用名',
        value: this.query,
        onInput: (value: string) => {
          this.query = value;
        }
      })

      if (this.error.length > 0) {
        Text(this.error)
          .fontSize(T.footnote)
          .fontColor(C.danger())
          .width('100%')
          .padding({ top: S.sm })
      }

      if (Breakpoints.isWide(this.bp)) {
        Row({ space: S.md }) {
          Column() {
            this.listPane()
          }
          .layoutWeight(4)
          .height('100%')

          Column() {
            if (this.selected() === undefined) {
              EmptyState({ symbol: 'history', title: '选一条记录', hint: '左侧列表里点开任意一条即可查看全文与参数' })
            } else {
              this.detailPane(this.selected() as HistoryEntry, true)
            }
          }
          .layoutWeight(5)
          .height('100%')
          .padding({ top: S.md })
        }
        .width('100%')
        .layoutWeight(1)
        .alignItems(VerticalAlign.Top)
      } else if (this.detailId.length > 0 && this.selected() !== undefined) {
        Scroll() {
          this.detailPane(this.selected() as HistoryEntry, false)
        }
        .scrollBar(BarState.Off)
        .layoutWeight(1)
        .align(Alignment.Top)
      } else {
        Column() {
          this.listPane()
        }
        .layoutWeight(1)
        .width('100%')
      }
    }
    .width('100%')
    .height('100%')
    .padding({ left: S.gutter(this.bp), right: S.gutter(this.bp) })
  }

  @Builder
  listPane() {
    if (this.filtered().length === 0) {
      EmptyState({
        symbol: 'history',
        title: this.query.length > 0 ? '没有匹配的记录' : '还没有听写记录',
        hint: this.query.length > 0 ? '换个关键词试试' : '在概览页点麦克风说第一句话'
      })
    } else {
      List({ space: S.sm }) {
        ForEach(this.filtered(), (entry: HistoryEntry) => {
          ListItem() {
            Row({ space: S.md }) {
              Column({ space: S.xs }) {
                Text(Fmt.preview(entry.finalText, Breakpoints.isWide(this.bp) ? 46 : 60))
                  .fontSize(T.body)
                  .fontColor(C.textPrimary())
                  .maxLines(2)
                  .textOverflow({ overflow: TextOverflow.Ellipsis })
                Row({ space: S.xs }) {
                  Text(Fmt.relative(entry.createdAt))
                    .fontSize(T.caption)
                    .fontColor(C.textTertiary())
                  if (modeLabel(entry.mode).length > 0) {
                    Pill({ text: modeLabel(entry.mode), tone: 'accent' })
                  }
                  if (entry.appName !== undefined && entry.appName !== null && entry.appName.length > 0) {
                    Pill({ text: entry.appName as string, tone: 'neutral' })
                  }
                  if (entry.errorCode !== undefined && entry.errorCode !== null) {
                    Pill({ text: '失败', tone: 'danger' })
                  }
                }
              }
              .alignItems(HorizontalAlign.Start)
              .layoutWeight(1)

              if (entry.durationMs !== undefined && entry.durationMs !== null && entry.durationMs > 0) {
                Text(Fmt.duration(entry.durationMs))
                  .fontSize(T.caption)
                  .fontColor(C.textTertiary())
              }
            }
            .width('100%')
            .padding(S.md)
            .backgroundColor(this.detailId === entry.id ? C.accentSoft() : C.surface())
            .borderRadius(R.md)
            .onClick(() => {
              Store.put<string>('detailId', this.detailId === entry.id ? '' : entry.id);
            })
          }
        }, (entry: HistoryEntry) => entry.id)
      }
      .width('100%')
      .height('100%')
      .scrollBar(BarState.Off)
      .lanes(Breakpoints.isWide(this.bp) ? 1 : 1)
    }
  }

  @Builder
  detailPane(entry: HistoryEntry, wide: boolean) {
    Column({ space: S.md }) {
      Card() {
        Column({ space: S.sm }) {
          Row({ space: S.sm }) {
            Text(Fmt.relative(entry.createdAt))
              .fontSize(T.callout)
              .fontColor(C.textSecondary())
              .layoutWeight(1)
            if (!wide) {
              GhostButton({
                label: '返回列表',
                onTap: () => Store.put<string>('detailId', '')
              })
            }
          }
          .width('100%')

          Text(entry.finalText)
            .fontSize(T.body)
            .fontColor(C.textPrimary())
            .lineHeight(25)
            .width('100%')
            .copyOption(CopyOptions.LocalDevice)
        }
        .width('100%')
      }

      if (entry.asrTranscript !== undefined && entry.asrTranscript !== null &&
        entry.asrTranscript.length > 0 && entry.asrTranscript !== entry.finalText) {
        Card() {
          Column({ space: S.xs }) {
            SectionHeader({ title: '识别原文', hint: '润色之前的 ASR 输出，用来区分听错还是改写' })
            Text(entry.asrTranscript as string)
              .fontSize(T.callout)
              .fontColor(C.textSecondary())
              .lineHeight(22)
              .width('100%')
              .copyOption(CopyOptions.LocalDevice)
          }
          .width('100%')
        }
      }

      Card() {
        Column() {
          SectionHeader({ title: '参数' })
          StatusRow({ label: '模式', value: modeLabel(entry.mode), showChevron: false })
          StatusRow({ label: '时长', value: Fmt.duration(entry.durationMs), showChevron: false })
          StatusRow({ label: '转写', value: this.latency(entry.asrProvider, entry.asrMs), showChevron: false })
          StatusRow({ label: '润色', value: this.latency(entry.llmProvider, entry.polishMs), showChevron: false })
          StatusRow({ label: '应用', value: entry.appName ?? '—', showChevron: false })
          StatusRow({
            label: '写入',
            value: entry.insertStatus ?? '—',
            showChevron: false,
            showDivider: entry.errorCode === undefined || entry.errorCode === null
          })
          if (entry.errorCode !== undefined && entry.errorCode !== null) {
            StatusRow({ label: '错误', value: entry.errorCode as string, showChevron: false, showDivider: false })
          }
        }
        .width('100%')
      }

      Row({ space: S.sm }) {
        if (entry.rawTranscript !== undefined && entry.rawTranscript.length > 0 &&
          entry.rawTranscript !== entry.finalText) {
          GhostButton({
            label: '复制原文',
            onTap: () => {
              Api.copyText(entry.asrTranscript !== undefined && entry.asrTranscript !== null &&
                entry.asrTranscript.length > 0 ? entry.asrTranscript as string : entry.rawTranscript as string);
            }
          })
        }
        GhostButton({
          label: this.busy ? '重润色中…' : '重新润色',
          symbol: 'sparkles',
          usable: !this.busy,
          onTap: () => {
            this.repolish(entry);
          }
        })
        GhostButton({
          label: '复制全文',
          onTap: () => {
            Api.copyText(entry.finalText);
          }
        })
        if (entry.hasAudioRecording === true) {
          GhostButton({
            label: this.playing ? '停止播放' : '播放录音',
            symbol: 'play',
            onTap: () => {
              this.togglePlayback(entry);
            }
          })
          GhostButton({
            label: this.busy ? '重新转写中…' : '重新转写',
            symbol: 'refresh',
            usable: !this.busy,
            onTap: () => {
              this.retranscribe(entry.id);
            }
          })
        }
        GhostButton({
          label: '删除',
          symbol: 'trash',
          onTap: () => {
            this.remove(entry.id);
          }
        })
      }
      .width('100%')
      .padding({ bottom: S.xxl })
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
  }

  private latency(provider: string | null | undefined, ms: number | null | undefined): string {
    const name: string = provider === undefined || provider === null || provider.length === 0 ? '—' : provider;
    if (ms === undefined || ms === null || ms <= 0) return name;
    return name + ' · ' + ms.toString() + ' ms';
  }
}
