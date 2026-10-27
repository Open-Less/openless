import { Breakpoints } from '../common/Breakpoint';
import { Labels } from '../common/Labels';
import { C, R, S, T } from '../common/Tokens';
import { Card, GhostButton, InfoRow, ListRow, Pill, SectionHeader, ToggleRow } from '../components/Common';
import { symbolOf } from '../components/Icons';
import { Nav, NavItem } from '../components/Nav';
import { Api, NetworkState } from '../data/Api';
import {
  CloudSyncStatus,
  HistoryEntry,
  PlatformCapabilities,
  ProviderDescriptor,
  SettingsSnapshot,
  StartupSnapshot,
  UserPreferences
} from '../data/Types';

const POLISH_MODES: string[] = ['raw', 'polish', 'translate', 'structured'];

/**
 * Settings: the single place that reads and writes `UserPreferences`.
 *
 * Every control writes through `update_setting_fields`, so the native UI and the
 * rest of the app share one preference document (no local shadow state). Wide
 * layouts show two columns; compact layouts stack the same groups.
 */
/** Live microphone level, fed by the capability layer through AppStorage. */
@Component
struct MicLevelBar {
  @StorageLink('micLevel') level: number = 0;

  build() {
    Row() {
      Row()
        .width(Math.max(6, Math.min(1, this.level) * 120))
        .height(8)
        .backgroundColor(C.accent())
        .borderRadius(R.pill)
    }
    .width(120)
    .height(8)
    .backgroundColor(C.sunken())
    .borderRadius(R.pill)
    .animation({ duration: 120, curve: Curve.EaseOut })
  }
}

@Component
export struct SettingsPage {
  @StorageLink('bp') bp: string = 'md';
  @StorageLink('backendRevision') revision: number = 0;

  @State private prefs: UserPreferences = {};
  @State private microphones: string[] = [];
  @State private network: string = '检测中…';
  @State private syncStatus: string = '未配置';
  @State private capabilities: PlatformCapabilities = { platform: 'harmony' };
  @State private contract: string = '';
  @State private historyCount: number = 0;
  @State private error: string = '';
  @State private busy: boolean = false;
  @State private asrOpen: boolean = false;
  @State private micTesting: boolean = false;
  @State private logPath: string = '';
  @State private syncLogin: string = '';
  @State private llmOpen: boolean = false;
  @State private asrProviders: ProviderDescriptor[] = [];
  @State private llmProviders: ProviderDescriptor[] = [];

  aboutToAppear(): void {
    this.reload();
  }

  private async reload(): Promise<void> {
    try {
      const snapshot: SettingsSnapshot = await Api.settings();
      this.prefs = snapshot.preferences;
      const startup: StartupSnapshot = await Api.startupSnapshot();
      this.contract = startup.contractVersion ?? '';
      this.capabilities = await Api.platformCapabilities();
      const network: NetworkState = await Api.network();
      this.network = network.online ? '已连接' : '未连接';
      this.microphones = await Api.microphoneDevices();
      const history: HistoryEntry[] = await Api.history();
      this.historyCount = history.length;
      try {
        const sync: CloudSyncStatus = await Api.cloudSyncStatus();
        this.syncStatus = sync.loggedIn === true ? '已登录 ' + (sync.account ?? '') : '未登录';
      } catch (error) {
        this.syncStatus = '未启用';
      }
      this.asrProviders = await Api.providerDescriptors('asr');
      this.llmProviders = await Api.providerDescriptors('llm');
      this.error = '';
    } catch (error) {
      this.error = (error as Error).message;
    }
  }

  private async writeBool(pointer: string, value: boolean): Promise<void> {
    this.busy = true;
    try {
      const snapshot: SettingsSnapshot = await Api.writeBool(pointer, value);
      this.prefs = snapshot.preferences;
    } catch (error) {
      this.error = (error as Error).message;
    } finally {
      this.busy = false;
    }
  }

  private async writeString(pointer: string, value: string): Promise<void> {
    this.busy = true;
    try {
      const snapshot: SettingsSnapshot = await Api.writeString(pointer, value);
      this.prefs = snapshot.preferences;
    } catch (error) {
      this.error = (error as Error).message;
    } finally {
      this.busy = false;
    }
  }

  private async writeNumber(pointer: string, value: number): Promise<void> {
    this.busy = true;
    try {
      const snapshot: SettingsSnapshot = await Api.writeNumber(pointer, value);
      this.prefs = snapshot.preferences;
    } catch (error) {
      this.error = (error as Error).message;
    } finally {
      this.busy = false;
    }
  }

  private async activateAsr(provider: string): Promise<void> {
    this.busy = true;
    try {
      const snapshot: SettingsSnapshot = await Api.activateAsrProvider(provider);
      this.prefs = snapshot.preferences;
      this.asrOpen = false;
    } catch (error) {
      this.error = (error as Error).message;
    } finally {
      this.busy = false;
    }
  }

  private async activateLlm(provider: string): Promise<void> {
    this.busy = true;
    try {
      const snapshot: SettingsSnapshot = await Api.activateLlmProvider(provider);
      this.prefs = snapshot.preferences;
      this.llmOpen = false;
    } catch (error) {
      this.error = (error as Error).message;
    } finally {
      this.busy = false;
    }
  }

  private async toggleMicTest(): Promise<void> {
    try {
      if (this.micTesting) {
        await Api.stopMicrophoneMonitor();
        this.micTesting = false;
      } else {
        await Api.startMicrophoneMonitor();
        this.micTesting = true;
      }
    } catch (error) {
      this.error = (error as Error).message;
      this.micTesting = false;
    }
  }

  private async exportLog(): Promise<void> {
    try {
      this.logPath = await Api.exportErrorLog('');
    } catch (error) {
      this.error = (error as Error).message;
    }
  }

  private microphoneValue(): string {
    const name: string | undefined = this.prefs.microphoneDeviceName;
    if (name === undefined || name.length === 0) return '系统默认';
    return name;
  }

  private pickMicrophone(name: string): void {
    this.writeString('/microphoneDeviceName', name);
  }

  build() {
    Column() {
      Row({ space: S.md }) {
        Column({ space: S.xxs }) {
          Text('设置')
            .fontSize(T.display)
            .fontWeight(FontWeight.Medium)
            .fontColor(C.textPrimary())
          Text('所有配置都在本机，凭据放在系统密钥库')
            .fontSize(T.footnote)
            .fontColor(C.textTertiary())
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)

        GhostButton({
          label: '重新读取',
          symbol: 'refresh',
          onTap: () => {
            this.reload();
          }
        })
      }
      .width('100%')
      .padding({ top: S.lg, bottom: S.md })

      if (this.error.length > 0) {
        Text(this.error)
          .fontSize(T.footnote)
          .fontColor(C.danger())
          .padding(S.sm)
          .width('100%')
          .backgroundColor(C.dangerSoft())
          .borderRadius(R.sm)
      }

      Scroll() {
        if (Breakpoints.isWide(this.bp)) {
          Row({ space: S.md }) {
            Column({ space: S.md }) {
              this.transcriptionCard()
              this.polishCard()
              this.inputCard()
              this.appearanceCard()
            }
            .layoutWeight(1)

            Column({ space: S.md }) {
              this.privacyCard()
              this.syncCard()
              this.aboutCard()
            }
            .layoutWeight(1)
          }
          .width('100%')
          .alignItems(VerticalAlign.Top)
        } else {
          Column({ space: S.md }) {
            this.toolsCard()
            this.transcriptionCard()
            this.polishCard()
            this.inputCard()
            this.appearanceCard()
            this.privacyCard()
            this.syncCard()
            this.aboutCard()
          }
          .width('100%')
        }
      }
      .scrollBar(BarState.Off)
      .layoutWeight(1)
      .align(Alignment.Top)
    }
    .width('100%')
    .height('100%')
    .padding({ left: S.gutter(this.bp), right: S.gutter(this.bp) })
  }

  @Builder
  transcriptionCard() {
    Card() {
      Column() {
        SectionHeader({ title: '转写', hint: '决定语音变成文字的方式' })
        ListRow({
          symbol: 'localasr',
          title: '转写引擎',
          value: Labels.provider(this.prefs.activeAsrProvider),
          onTap: () => {
            this.asrOpen = !this.asrOpen;
          }
        })
        if (this.asrOpen) {
          this.providerList(this.asrProviders, this.prefs.activeAsrProvider ?? '', true)
        }
        InfoRow({
          label: '处理管线',
          value: Labels.pipeline(this.prefs.pipelineMode),
          hint: '全模态把转写与润色合成一次调用，延迟更低',
          showDivider: false
        })
        Row({ space: S.sm }) {
          ForEach(['traditional', 'multimodal'], (id: string) => {
            this.choiceChip(Labels.pipeline(id), this.prefs.pipelineMode === id, () => {
              this.writeString('/pipelineMode', id);
            })
          }, (id: string) => 'pipe-' + id)
        }
        .width('100%')
        .padding({ top: S.sm })
      }
      .width('100%')
    }
  }

  @Builder
  polishCard() {
    Card() {
      Column() {
        SectionHeader({ title: '润色', hint: '决定文字最终长什么样' })
        ListRow({
          symbol: 'style',
          title: '润色模型',
          value: Labels.provider(this.prefs.activeLlmProvider),
          onTap: () => {
            this.llmOpen = !this.llmOpen;
          }
        })
        if (this.llmOpen) {
          this.providerList(this.llmProviders, this.prefs.activeLlmProvider ?? '', false)
        }
        ToggleRow({
          label: '深度思考',
          hint: '更慢，但长句改写更稳',
          checked: this.prefs.llmThinkingEnabled === true,
          onToggle: (value: boolean) => {
            this.writeBool('/llmThinkingEnabled', value);
          }
        })
        Text('默认模式')
          .fontSize(T.callout)
          .fontColor(C.textPrimary())
          .width('100%')
          .padding({ top: S.sm, bottom: S.xs })
        Row({ space: S.sm }) {
          ForEach(POLISH_MODES, (mode: string) => {
            this.choiceChip(Labels.polishMode(mode), this.prefs.defaultMode === mode, () => {
              this.writeString('/defaultMode', mode);
            })
          }, (mode: string) => 'mode-' + mode)
        }
        .width('100%')
        .padding({ bottom: S.xs })
      }
      .width('100%')
    }
  }

  @Builder
  inputCard() {
    Card() {
      Column() {
        SectionHeader({ title: '输入', hint: '麦克风与录音行为' })
        InfoRow({
          label: '麦克风',
          value: this.microphoneValue(),
          hint: this.microphones.length.toString() + ' 个输入设备',
          showChevron: true,
          showDivider: true
        })
        Row({ space: S.sm }) {
          this.choiceChip('系统默认', (this.prefs.microphoneDeviceName ?? '').length === 0, () => this.pickMicrophone(''))
          ForEach(this.microphones, (name: string) => {
            this.choiceChip(name, this.prefs.microphoneDeviceName === name, () => this.pickMicrophone(name))
          }, (name: string) => 'mic-' + name)
        }
        .width('100%')
        .padding({ bottom: S.sm })

        Row({ space: S.sm }) {
          GhostButton({
            label: this.micTesting ? '停止电平测试' : '测试麦克风电平',
            symbol: 'mic',
            onTap: () => {
              this.toggleMicTest();
            }
          })
          if (this.micTesting) {
            MicLevelBar()
          }
        }
        .width('100%')
        .padding({ bottom: S.sm })

        ToggleRow({
          label: '低延迟听写',
          hint: '边录边转写，首字更快',
          checked: this.prefs.lowLatencyDictationEnabled === true,
          onToggle: (value: boolean) => {
            this.writeBool('/lowLatencyDictationEnabled', value);
          }
        })
        ToggleRow({
          label: '稳定转写',
          hint: '牺牲一点延迟换取更稳定的长句结果',
          checked: this.prefs.stableTranscriptionEnabled === true,
          onToggle: (value: boolean) => {
            this.writeBool('/stableTranscriptionEnabled', value);
          }
        })
        ToggleRow({
          label: '音频增强',
          checked: this.prefs.audioEnhancementEnabled === true,
          onToggle: (value: boolean) => {
            this.writeBool('/audioEnhancementEnabled', value);
          }
        })
        ToggleRow({
          label: '录音提示音',
          checked: this.prefs.audioCueOnRecord === true,
          onToggle: (value: boolean) => {
            this.writeBool('/audioCueOnRecord', value);
          }
        })
        ToggleRow({
          label: '录音时静音其他音频',
          checked: this.prefs.muteDuringRecording === true,
          onToggle: (value: boolean) => {
            this.writeBool('/muteDuringRecording', value);
          }
        })
        ToggleRow({
          label: '静音自动停止',
          hint: '说完停顿几秒自动结束录音',
          checked: this.prefs.silenceAutoStopEnabled === true,
          showDivider: false,
          onToggle: (value: boolean) => {
            this.writeBool('/silenceAutoStopEnabled', value);
          }
        })
        if (this.prefs.silenceAutoStopEnabled === true) {
          Row({ space: S.md }) {
            Text('停顿阈值')
              .fontSize(T.callout)
              .fontColor(C.textSecondary())
              .layoutWeight(1)
            Text((this.prefs.silenceAutoStopSeconds ?? 2).toFixed(1) + ' 秒')
              .fontSize(T.callout)
              .fontColor(C.textPrimary())
            GhostButton({
              label: '−',
              onTap: () => this.writeNumber('/silenceAutoStopSeconds', Math.max(1, (this.prefs.silenceAutoStopSeconds ?? 2) - 0.5))
            })
            GhostButton({
              label: '+',
              onTap: () => this.writeNumber('/silenceAutoStopSeconds', Math.min(10, (this.prefs.silenceAutoStopSeconds ?? 2) + 0.5))
            })
          }
          .width('100%')
          .padding({ top: S.sm })
        }
      }
      .width('100%')
    }
  }

  /** Everything that used to be a top-level destination, kept one tap deeper. */
  @Builder
  toolsCard() {
    Card() {
      Column() {
        SectionHeader({ title: '内容与工具', hint: '市场、翻译、问答、速记与规则' })
        ForEach(Nav.tools(), (item: NavItem, index: number) => {
          ListRow({
            symbol: item.id,
            title: item.label,
            showChevron: true,
            showDivider: index < Nav.tools().length - 1,
            onTap: () => Nav.go(item.id)
          })
        }, (item: NavItem) => 'tool-' + item.id)
      }
      .width('100%')
    }
  }

  @Builder
  appearanceCard() {
    Card() {
      Column({ space: S.sm }) {
        SectionHeader({ title: '外观', hint: '跟随系统时由深浅色模式自动切换' })
        Row({ space: S.sm }) {
          ForEach(['system', 'light', 'dark'], (option: string) => {
            this.choiceChip(option === 'system' ? '跟随系统' : (option === 'light' ? '浅色' : '深色'),
              (this.prefs.theme ?? 'system') === option, () => {
                this.writeString('/theme', option);
              })
          }, (option: string) => 'theme-' + option)
        }
        .width('100%')
      }
      .width('100%')
    }
  }

  @Builder
  privacyCard() {
    Card() {
      Column() {
        SectionHeader({ title: '数据与隐私', hint: '录音、历史与调试产物都留在设备上' })
        ToggleRow({
          label: '保留录音以便重新转写',
          hint: '开启后历史条目可以重新识别',
          checked: this.prefs.recordAudioForDebug === true,
          onToggle: (value: boolean) => {
            this.writeBool('/recordAudioForDebug', value);
          }
        })
        ToggleRow({
          label: '启用纠正规则',
          checked: this.prefs.correctionRulesEnabled !== false,
          onToggle: (value: boolean) => {
            this.writeBool('/correctionRulesEnabled', value);
          }
        })
        InfoRow({
          label: '纠正规则',
          value: '管理替换规则',
          showChevron: true,
          onTap: () => Nav.go(Nav.CORRECTIONS)
        })
        InfoRow({
          label: '历史记录',
          value: this.historyCount.toString() + ' 条',
          showChevron: true,
          showDivider: false,
          onTap: () => Nav.go(Nav.HISTORY)
        })
      }
      .width('100%')
    }
  }

  @Builder
  syncCard() {
    Card() {
      Column() {
        SectionHeader({ title: '云同步', hint: '端到端加密，密钥不出设备' })
        InfoRow({
          label: '账号',
          value: this.syncStatus,
          showDivider: true
        })
        InfoRow({
          label: '网络',
          value: this.network,
          showDivider: true
        })
        InfoRow({
          label: '上传本机偏好',
          value: '端到端加密，口令不出设备',
          showChevron: true,
          showDivider: false,
          onTap: () => {
            this.error = '云同步登录需要口令输入，桌面端完成后再回到这里';
          }
        })
      }
      .width('100%')
    }
  }

  @Builder
  aboutCard() {
    Card() {
      Column() {
        SectionHeader({ title: '关于', hint: '鸿蒙原生版' })
        InfoRow({ label: '应用', value: 'OpenLess 鸿蒙原生版', showDivider: true })
        InfoRow({ label: '后端契约', value: this.contract.length > 0 ? this.contract : '—', showDivider: true })
        InfoRow({ label: '平台', value: this.capabilities.platform, showDivider: true })
        InfoRow({
          label: '离线听写',
          value: this.capabilities.supportsLocalAsr === true ? '支持' : '不支持（需联网）',
          showDivider: true
        })
        InfoRow({
          label: '输入法',
          value: this.capabilities.supportsImeInput === true ? '已接入系统输入法' : '未接入',
          showDivider: true
        })
        InfoRow({
          label: '导出诊断日志',
          value: this.logPath.length > 0 ? this.logPath : '点此导出',
          showChevron: this.logPath.length === 0,
          showDivider: false,
          onTap: () => {
            this.exportLog();
          }
        })
      }
      .width('100%')
    }
  }

  @Builder
  providerList(providers: ProviderDescriptor[], active: string, asr: boolean) {
    Column({ space: S.xs }) {
      if (providers.length === 0) {
        Text('暂时读不到供应商列表')
          .fontSize(T.caption)
          .fontColor(C.textTertiary())
      } else {
        ForEach(providers, (descriptor: ProviderDescriptor) => {
          Row({ space: S.sm }) {
            Text(descriptor.providerType)
              .fontSize(T.callout)
              .fontColor(descriptor.providerType === active ? C.accent() : C.textPrimary())
              .layoutWeight(1)
            if (descriptor.providerType === active) {
              SymbolGlyph(symbolOf('check'))
                .fontSize(16)
                .fontColor([C.accent()])
            }
          }
          .width('100%')
          .height(40)
          .padding({ left: S.md, right: S.md })
          .backgroundColor(descriptor.providerType === active ? C.accentSoft() : C.sunken())
          .borderRadius(R.sm)
          .onClick(() => {
            if (asr) {
              this.activateAsr(descriptor.providerType);
            } else {
              this.activateLlm(descriptor.providerType);
            }
          })
        }, (descriptor: ProviderDescriptor) => descriptor.kind + '-' + descriptor.providerType)
      }
    }
    .width('100%')
    .padding({ bottom: S.sm })
  }

  @Builder
  choiceChip(label: string, selected: boolean, onTap: () => void) {
    Text(label)
      .fontSize(T.footnote)
      .fontColor(selected ? C.textOnAccent() : C.textSecondary())
      .padding({ left: S.md, right: S.md, top: S.xs, bottom: S.xs })
      .backgroundColor(selected ? C.accent() : C.sunken())
      .borderRadius(R.pill)
      .onClick(() => onTap())
  }
}
