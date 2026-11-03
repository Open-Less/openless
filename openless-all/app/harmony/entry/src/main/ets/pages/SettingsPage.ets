import { common } from '@kit.AbilityKit';
import { Appearance } from '../common/Appearance';
import { Labels } from '../common/Labels';
import { Layout, Leading, Motion, Palette, Radius, Space, Type } from '../common/Theme';
import {
  EmptyHint,
  ErrorNote,
  Group,
  PageScaffold,
  PrimaryButton,
  RowItem,
  SectionTitle,
  Segmented,
  SettingSwitch,
  SheetTitle,
  TextButton,
  TonalButton,
  notify
} from '../components/Kit';
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

const THEME_IDS: string[] = ['system', 'light', 'dark'];
const THEME_LABELS: string[] = ['跟随系统', '浅色', '深色'];
const PIPELINE_IDS: string[] = ['traditional', 'multimodal'];
const PIPELINE_LABELS: string[] = ['两段式', '全模态'];
const POLISH_MODES: string[] = ['raw', 'polish', 'translate', 'structured'];

/**
 * Settings v2: the one place that reads and writes `UserPreferences`.
 *
 * Every control writes through `update_setting_fields`, so the native UI and the
 * rest of the app share a single preference document (no local shadow state).
 * Sections are the same vocabulary as the rest of the app - `Group` rows for
 * values and switches, `Segmented` for choices, `bindSheet` for pickers - and the
 * appearance choice really applies (`Appearance.apply`), because the preference
 * is useless on its own.
 *
 * Wide windows put the frequent sections next to the occasional ones; compact
 * windows stack the same sections in reading order.
 */
@Component
export struct SettingsPage {
  @StorageLink('bp') bp: string = 'md';
  @StorageLink('backendRevision') revision: number = 0;
  @StorageLink('micLevel') micLevel: number = 0;

  @State private prefs: UserPreferences = {};
  @State private microphones: string[] = [];
  @State private network: string = '检测中…';
  @State private syncStatus: string = '未配置';
  @State private syncLast: string = '';
  @State private syncNote: string = '';
  @State private syncRevision: number = 0;
  @State private capabilities: PlatformCapabilities = { platform: 'harmony' };
  @State private contract: string = '';
  @State private historyCount: number = 0;
  @State private error: string = '';
  @State private busy: boolean = false;
  @State private asrOpen: boolean = false;
  @State private llmOpen: boolean = false;
  @State private micOpen: boolean = false;
  @State private micTesting: boolean = false;
  @State private logPath: string = '';
  @State private syncing: boolean = false;
  @State private asrProviders: ProviderDescriptor[] = [];
  @State private llmProviders: ProviderDescriptor[] = [];

  aboutToAppear(): void {
    this.reload();
  }

  aboutToDisappear(): void {
    this.stopMicTest();
  }

  /** Leaving the page must not leave the level monitor holding the microphone. */
  private async stopMicTest(): Promise<void> {
    if (!this.micTesting) return;
    try {
      await Api.stopMicrophoneMonitor();
    } catch (error) {
      console.warn('[OpenLess Settings] microphone monitor not stopped');
    }
    this.micTesting = false;
  }

  // ── Data ────────────────────────────────────────────────────────────────────

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
      await this.loadSyncStatus();
      this.asrProviders = await Api.providerDescriptors('asr');
      this.llmProviders = await Api.providerDescriptors('llm');
      this.error = '';
    } catch (error) {
      this.error = (error as Error).message;
    }
  }

  private async loadSyncStatus(): Promise<void> {
    try {
      const sync: CloudSyncStatus = await Api.cloudSyncStatus();
      this.syncStatus = sync.loggedIn === true ? '已登录 ' + (sync.account ?? '') : '未登录';
      this.syncRevision = sync.revision ?? 0;
      this.syncLast = sync.lastSyncAt !== undefined && sync.lastSyncAt !== null ? sync.lastSyncAt : '';
      this.syncNote = '';
    } catch (error) {
      this.syncStatus = '未启用';
      this.syncLast = '';
      this.syncRevision = 0;
    }
  }

  /** Writes one boolean preference; the snapshot comes back so the UI never guesses. */
  private async writeBool(pointer: string, value: boolean): Promise<boolean> {
    this.busy = true;
    try {
      const snapshot: SettingsSnapshot = await Api.writeBool(pointer, value);
      this.prefs = snapshot.preferences;
      this.error = '';
      return true;
    } catch (error) {
      this.error = (error as Error).message;
      return false;
    } finally {
      this.busy = false;
    }
  }

  private async writeString(pointer: string, value: string): Promise<boolean> {
    this.busy = true;
    try {
      const snapshot: SettingsSnapshot = await Api.writeString(pointer, value);
      this.prefs = snapshot.preferences;
      this.error = '';
      return true;
    } catch (error) {
      this.error = (error as Error).message;
      return false;
    } finally {
      this.busy = false;
    }
  }

  private async writeNumber(pointer: string, value: number): Promise<boolean> {
    this.busy = true;
    try {
      const snapshot: SettingsSnapshot = await Api.writeNumber(pointer, value);
      this.prefs = snapshot.preferences;
      this.error = '';
      return true;
    } catch (error) {
      this.error = (error as Error).message;
      return false;
    } finally {
      this.busy = false;
    }
  }

  private async applyMode(mode: string): Promise<void> {
    this.busy = true;
    try {
      const snapshot: SettingsSnapshot = await Api.setDefaultPolishMode(mode);
      this.prefs = snapshot.preferences;
      notify('默认模式已切换为' + Labels.polishMode(mode));
      this.error = '';
    } catch (error) {
      this.error = (error as Error).message;
    } finally {
      this.busy = false;
    }
  }

  private async applyPipeline(mode: string): Promise<void> {
    const saved: boolean = await this.writeString('/pipelineMode', mode);
    if (saved) {
      notify('处理管线已切换为' + Labels.pipeline(mode));
    }
  }

  /**
   * Appearance is a real preference: persist it, then push it into the
   * application context so light/dark follows the user instead of the system.
   */
  private async applyTheme(theme: string): Promise<void> {
    if ((this.prefs.theme ?? 'system') === theme) return;
    const saved: boolean = await this.writeString('/theme', theme);
    if (!saved) {
      notify('外观切换失败');
      return;
    }
    if (this.applyCurrentAppearance()) {
      notify('外观已切换为' + this.themeLabel(theme));
    } else {
      notify('外观已保存，重启后生效');
    }
  }

  /** Pushes the saved 跟随系统/浅色/深色 preference into the ability context. */
  private applyCurrentAppearance(): boolean {
    const context = this.getUIContext().getHostContext();
    if (context === undefined) return false;
    Appearance.apply(context as common.UIAbilityContext, this.prefs.theme ?? 'system');
    return true;
  }

  private async activateAsr(provider: string): Promise<void> {
    this.busy = true;
    try {
      const snapshot: SettingsSnapshot = await Api.activateAsrProvider(provider);
      this.prefs = snapshot.preferences;
      this.asrOpen = false;
      notify('转写引擎已切换为' + Labels.provider(provider));
      this.error = '';
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
      notify('润色模型已切换为' + Labels.provider(provider));
      this.error = '';
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
        notify('已停止电平检测');
      } else {
        await Api.startMicrophoneMonitor();
        this.micTesting = true;
      }
      this.error = '';
    } catch (error) {
      this.error = (error as Error).message;
      this.micTesting = false;
    }
  }

  private async refreshMicrophones(): Promise<void> {
    try {
      this.microphones = await Api.microphoneDevices();
      notify('已重新检测输入设备');
      this.error = '';
    } catch (error) {
      this.error = (error as Error).message;
    }
  }

  private async exportLog(): Promise<void> {
    try {
      this.logPath = await Api.exportErrorLog('');
      notify('诊断日志已导出');
      this.error = '';
    } catch (error) {
      this.error = (error as Error).message;
      notify('导出诊断日志失败');
    }
  }

  // ── Cloud sync ──────────────────────────────────────────────────────────────

  private async syncUpload(): Promise<void> {
    this.syncing = true;
    this.syncNote = '正在上传…';
    try {
      const snapshot: SettingsSnapshot = await Api.settings();
      await Api.cloudSyncUpload(this.syncRevision, JSON.stringify(snapshot.preferences));
      await this.reload();
      this.syncNote = '已上传，等待其他设备合并';
      notify('偏好已上传');
    } catch (error) {
      this.syncNote = '上传失败：' + (error as Error).message;
    } finally {
      this.syncing = false;
    }
  }

  private async syncRestore(): Promise<void> {
    this.syncing = true;
    this.syncNote = '正在恢复…';
    try {
      await Api.cloudSyncRestore();
      await this.reload();
      this.applyCurrentAppearance();
      this.syncNote = '已从云端恢复偏好';
      notify('已从云端恢复偏好');
    } catch (error) {
      this.syncNote = '恢复失败：' + (error as Error).message;
    } finally {
      this.syncing = false;
    }
  }

  private async syncDelete(): Promise<void> {
    this.syncing = true;
    this.syncNote = '正在删除…';
    try {
      await Api.cloudSyncDelete(this.syncRevision);
      await this.reload();
      this.syncNote = '云端快照已删除';
      notify('云端快照已删除');
    } catch (error) {
      this.syncNote = '删除失败：' + (error as Error).message;
    } finally {
      this.syncing = false;
    }
  }

  // ── Derived labels ──────────────────────────────────────────────────────────

  private isWide(): boolean {
    return this.bp === Layout.LG || this.bp === Layout.XL;
  }

  private versionLabel(): string {
    const raw: string | undefined = AppStorage.get<string>('appVersion');
    if (raw === undefined || raw.length === 0) return '—';
    return 'v' + raw;
  }

  private subtitleText(): string {
    if (this.contract.length > 0) {
      return this.versionLabel() + ' · ' + this.network + ' · 契约 ' + this.contract;
    }
    return this.versionLabel() + ' · ' + this.network;
  }

  private themeLabel(theme: string): string {
    if (theme === 'light') return '浅色';
    if (theme === 'dark') return '深色';
    return '跟随系统';
  }

  private themeIndex(): number {
    const current: string = this.prefs.theme ?? 'system';
    for (let index = 0; index < THEME_IDS.length; index++) {
      if (THEME_IDS[index] === current) return index;
    }
    return 0;
  }

  private pipelineIndex(): number {
    return this.prefs.pipelineMode === 'multimodal' ? 1 : 0;
  }

  private polishLabels(): string[] {
    const out: string[] = [];
    for (let index = 0; index < POLISH_MODES.length; index++) {
      out.push(Labels.polishMode(POLISH_MODES[index]));
    }
    return out;
  }

  private polishIndex(): number {
    const current: string = this.prefs.defaultMode ?? 'polish';
    for (let index = 0; index < POLISH_MODES.length; index++) {
      if (POLISH_MODES[index] === current) return index;
    }
    return 1;
  }

  private microphoneValue(): string {
    const name: string | undefined = this.prefs.microphoneDeviceName;
    if (name === undefined || name.length === 0) return '系统默认';
    return name;
  }

  private micOptions(): string[] {
    const options: string[] = ['系统默认'];
    for (let index = 0; index < this.microphones.length; index++) {
      options.push(this.microphones[index]);
    }
    return options;
  }

  private micIndex(): number {
    const name: string = this.prefs.microphoneDeviceName ?? '';
    if (name.length === 0) return 0;
    for (let index = 0; index < this.microphones.length; index++) {
      if (this.microphones[index] === name) return index + 1;
    }
    return 0;
  }

  private choiceItems(kind: string): string[] {
    if (kind === 'theme') return THEME_LABELS;
    if (kind === 'pipeline') return PIPELINE_LABELS;
    if (kind === 'mode') return this.polishLabels();
    return this.micOptions();
  }

  private choiceSelected(kind: string): number {
    if (kind === 'theme') return this.themeIndex();
    if (kind === 'pipeline') return this.pipelineIndex();
    if (kind === 'mode') return this.polishIndex();
    return this.micIndex();
  }

  private silenceSeconds(): number {
    return this.prefs.silenceAutoStopSeconds ?? 2;
  }

  private silenceLabel(): string {
    return this.silenceSeconds().toFixed(1) + ' 秒';
  }

  private levelWidth(): number {
    const level: number = Math.min(1, Math.max(0, this.micLevel));
    return Math.round(Math.max(4, level * 96));
  }

  private async pickMicrophone(name: string): Promise<void> {
    this.micOpen = false;
    const saved: boolean = await this.writeString('/microphoneDeviceName', name);
    if (saved) {
      notify(name.length === 0 ? '已改用系统默认麦克风' : '已切换麦克风：' + name);
    } else {
      notify('切换麦克风失败');
    }
  }

  private pickMicrophoneByIndex(index: number): void {
    if (index <= 0) {
      this.pickMicrophone('');
      return;
    }
    if (index - 1 < this.microphones.length) {
      this.pickMicrophone(this.microphones[index - 1]);
    }
  }

  private shiftSilence(delta: number): void {
    const next: number = Math.round(Math.min(10, Math.max(1, this.silenceSeconds() + delta)) * 2) / 2;
    this.writeNumber('/silenceAutoStopSeconds', next);
  }

  private applyChoice(kind: string, index: number): void {
    if (kind === 'theme') {
      this.applyTheme(THEME_IDS[index]);
      return;
    }
    if (kind === 'pipeline') {
      this.applyPipeline(PIPELINE_IDS[index]);
      return;
    }
    if (kind === 'mode') {
      this.applyMode(POLISH_MODES[index]);
      return;
    }
    this.pickMicrophoneByIndex(index);
  }

  // ── View ────────────────────────────────────────────────────────────────────

  build() {
    PageScaffold({
      title: '设置',
      subtitle: this.subtitleText(),
      actionLabel: '重新读取',
      actionSymbol: 'refresh',
      onAction: () => {
        this.reload();
      }
    }) {
      if (this.error.length > 0) {
        ErrorNote({
          message: this.error,
          onRetry: () => {
            this.reload();
          }
        })
      }

      if (this.isWide()) {
        Row({ space: Space.md }) {
          Column({ space: Space.md }) {
            this.appearanceSection()
            this.dictationSection()
            this.microphoneSection()
          }
          .layoutWeight(1)
          .alignItems(HorizontalAlign.Start)

          Column({ space: Space.md }) {
            this.cloudSection()
            this.toolsSection()
            this.aboutSection()
          }
          .layoutWeight(1)
          .alignItems(HorizontalAlign.Start)
        }
        .width('100%')
        .alignItems(VerticalAlign.Top)
      } else {
        Column({ space: Space.md }) {
          this.appearanceSection()
          this.dictationSection()
          this.microphoneSection()
          this.cloudSection()
          this.toolsSection()
          this.aboutSection()
        }
        .width('100%')
      }
    }
  }

  /** Section = heading + one grouped card; keeps every section the same rhythm. */
  @Builder
  appearanceSection() {
    Column({ space: Space.xs }) {
      SectionTitle({
        title: '外观',
        hint: this.busy ? '正在保存…' : '跟随系统时随浅色/深色模式自动切换'
      })
      Group() {
        this.choiceBlock('theme', '颜色模式', '立即应用到主界面与输入法面板', false)
      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
  }

  @Builder
  dictationSection() {
    Column({ space: Space.xs }) {
      SectionTitle({ title: '听写与引擎', hint: '决定语音变成文字的方式，以及文字最终的样子' })
      Group() {
        RowItem({
          symbol: 'localasr',
          title: '转写引擎',
          value: Labels.provider(this.prefs.activeAsrProvider),
          showChevron: true,
          onTap: () => {
            this.asrOpen = true;
          }
        })

        Row()
          .width(0)
          .height(0)
          .bindSheet($$this.asrOpen, this.asrSheet, {
            height: SheetSize.MEDIUM,
            dragBar: true,
            showClose: true,
            onDisappear: () => {
              this.asrOpen = false;
            }
          })

        RowItem({
          symbol: 'style',
          title: '润色模型',
          value: Labels.provider(this.prefs.activeLlmProvider),
          showChevron: true,
          onTap: () => {
            this.llmOpen = true;
          }
        })

        Row()
          .width(0)
          .height(0)
          .bindSheet($$this.llmOpen, this.llmSheet, {
            height: SheetSize.MEDIUM,
            dragBar: true,
            showClose: true,
            onDisappear: () => {
              this.llmOpen = false;
            }
          })

        this.choiceBlock('pipeline', '处理管线', '全模态把转写与润色合成一次调用，延迟更低', true)
        this.choiceBlock('mode', '默认润色模式', '听写结束后默认套用的处理方式', true)

        SettingSwitch({
          label: '深度思考',
          hint: '更慢，但长句改写更稳',
          checked: this.prefs.llmThinkingEnabled === true,
          onToggle: (value: boolean) => {
            this.writeBool('/llmThinkingEnabled', value);
          }
        })
        SettingSwitch({
          label: '低延迟听写',
          hint: '边录边转写，首字更快',
          checked: this.prefs.lowLatencyDictationEnabled === true,
          onToggle: (value: boolean) => {
            this.writeBool('/lowLatencyDictationEnabled', value);
          }
        })
        SettingSwitch({
          label: '稳定转写',
          hint: '牺牲一点延迟，换取更稳的长句结果',
          checked: this.prefs.stableTranscriptionEnabled === true,
          onToggle: (value: boolean) => {
            this.writeBool('/stableTranscriptionEnabled', value);
          }
        })
        SettingSwitch({
          label: '音频增强',
          hint: '开麦前做降噪与自动增益',
          checked: this.prefs.audioEnhancementEnabled === true,
          onToggle: (value: boolean) => {
            this.writeBool('/audioEnhancementEnabled', value);
          }
        })
        SettingSwitch({
          label: '保留录音以便重新转写',
          hint: '开启后历史条目可以重新识别',
          checked: this.prefs.recordAudioForDebug === true,
          onToggle: (value: boolean) => {
            this.writeBool('/recordAudioForDebug', value);
          }
        })
        SettingSwitch({
          label: '启用纠正规则',
          hint: '听写结束后按词典与规则做替换',
          checked: this.prefs.correctionRulesEnabled !== false,
          showDivider: false,
          onToggle: (value: boolean) => {
            this.writeBool('/correctionRulesEnabled', value);
          }
        })
      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
  }

  @Builder
  microphoneSection() {
    Column({ space: Space.xs }) {
      SectionTitle({ title: '麦克风', hint: '输入设备与录音时的行为' })
      Group() {
        RowItem({
          symbol: 'mic',
          title: '输入设备',
          subtitle: this.microphones.length.toString() + ' 个可用输入',
          value: this.microphoneValue(),
          showChevron: true,
          onTap: () => {
            this.micOpen = true;
          }
        })

        Row()
          .width(0)
          .height(0)
          .bindSheet($$this.micOpen, this.micSheet, {
            height: SheetSize.MEDIUM,
            dragBar: true,
            showClose: true,
            onDisappear: () => {
              this.micOpen = false;
            }
          })

        this.micLevelBlock()

        SettingSwitch({
          label: '录音提示音',
          hint: '开始与结束录音时各响一声',
          checked: this.prefs.audioCueOnRecord === true,
          onToggle: (value: boolean) => {
            this.writeBool('/audioCueOnRecord', value);
          }
        })
        SettingSwitch({
          label: '录音时静音其他音频',
          checked: this.prefs.muteDuringRecording === true,
          onToggle: (value: boolean) => {
            this.writeBool('/muteDuringRecording', value);
          }
        })
        SettingSwitch({
          label: '静音自动停止',
          hint: '说完停顿几秒自动结束录音',
          checked: this.prefs.silenceAutoStopEnabled === true,
          showDivider: this.prefs.silenceAutoStopEnabled === true,
          onToggle: (value: boolean) => {
            this.writeBool('/silenceAutoStopEnabled', value);
          }
        })
        if (this.prefs.silenceAutoStopEnabled === true) {
          this.silenceBlock()
        }
      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
  }

  @Builder
  cloudSection() {
    Column({ space: Space.xs }) {
      SectionTitle({ title: '云同步', hint: '端到端加密，密钥不出设备' })
      Group() {
        RowItem({
          title: '登录状态',
          value: this.syncStatus,
          showDivider: true
        })
        RowItem({
          title: '网络',
          value: this.network,
          showDivider: true
        })
        RowItem({
          title: '上次同步',
          value: this.syncLast.length > 0 ? this.syncLast : '—',
          subtitle: this.syncNote,
          showDivider: true
        })
        this.syncActionsBlock()
      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
  }

  @Builder
  toolsSection() {
    Column({ space: Space.xs }) {
      SectionTitle({ title: '工具', hint: '市场、翻译、问答、速记与规则' })
      Group() {
        ForEach(Nav.tools(), (item: NavItem) => {
          RowItem({
            symbol: item.id,
            title: item.label,
            showChevron: true,
            showDivider: true,
            onTap: () => Nav.go(item.id)
          })
        }, (item: NavItem) => 'tool-' + item.id)
        RowItem({
          symbol: 'history',
          title: '历史记录',
          value: this.historyCount.toString() + ' 条',
          showChevron: true,
          showDivider: false,
          onTap: () => Nav.go(Nav.HISTORY)
        })
      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
  }

  @Builder
  aboutSection() {
    Column({ space: Space.xs }) {
      SectionTitle({ title: '关于', hint: '鸿蒙原生版' })
      Group() {
        RowItem({
          title: '应用',
          value: 'OpenLess 鸿蒙原生版',
          showDivider: true
        })
        RowItem({
          title: '版本',
          value: this.versionLabel(),
          showDivider: true
        })
        RowItem({
          title: '后端契约',
          value: this.contract.length > 0 ? this.contract : '—',
          showDivider: true
        })
        RowItem({
          title: '平台',
          value: this.capabilities.platform,
          showDivider: true
        })
        RowItem({
          title: '离线听写',
          value: this.capabilities.supportsLocalAsr === true ? '支持' : '不支持（需联网）',
          showDivider: true
        })
        RowItem({
          title: '输入法',
          value: this.capabilities.supportsImeInput === true ? '已接入系统输入法' : '未接入',
          showDivider: true
        })
        RowItem({
          title: '导出诊断日志',
          subtitle: this.logPath.length > 0 ? this.logPath : '把最近的错误记录写成 JSON 文件',
          value: this.logPath.length > 0 ? '已导出' : '',
          showChevron: this.logPath.length === 0,
          showDivider: false,
          onTap: () => {
            this.exportLog();
          }
        })
      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
  }

  /** Segmented choice row inside a card: label, one-line hint, wrapped pills. */
  @Builder
  choiceBlock(kind: string, title: string, hint: string, showDivider: boolean) {
    Column() {
      Column({ space: Space.xs }) {
        Text(title)
          .fontSize(Type.body)
          .lineHeight(Leading.body)
          .fontColor(Palette.text1())
        if (hint.length > 0) {
          Text(hint)
            .fontSize(Type.caption)
            .lineHeight(Leading.caption)
            .fontColor(Palette.text3())
        }
        Segmented({
          items: this.choiceItems(kind),
          selected: this.choiceSelected(kind),
          onSelect: (index: number) => {
            this.applyChoice(kind, index);
          }
        })
      }
      .width('100%')
      .alignItems(HorizontalAlign.Start)
      .padding({ left: Space.md, right: Space.md, top: Space.sm, bottom: Space.sm })

      if (showDivider) {
        Divider()
          .color(Palette.line())
          .strokeWidth(0.5)
          .margin({ left: Space.md })
      }
    }
    .width('100%')
  }

  /** Live input level + the monitor toggle, so the mic choice can be verified. */
  @Builder
  micLevelBlock() {
    Column() {
      Row({ space: Space.sm }) {
        Column({ space: Space.xxs }) {
          Text('麦克风电平')
            .fontSize(Type.body)
            .lineHeight(Leading.body)
            .fontColor(Palette.text1())
          Text(this.micTesting ? '正在采集，说一句话看条形' : '测一下，确认设备真的在收音')
            .fontSize(Type.caption)
            .lineHeight(Leading.caption)
            .fontColor(Palette.text3())
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)

        if (this.micTesting) {
          this.levelMeter()
        }
        TextButton({
          label: this.micTesting ? '停止' : '测试',
          symbol: 'mic',
          onTap: () => {
            this.toggleMicTest();
          }
        })
      }
      .width('100%')
      .padding({ left: Space.md, right: Space.md, top: Space.sm, bottom: Space.sm })

      Divider()
        .color(Palette.line())
        .strokeWidth(0.5)
        .margin({ left: Space.md })
    }
    .width('100%')
  }

  @Builder
  levelMeter() {
    Stack({ alignContent: Alignment.Start }) {
      Row()
        .width(96)
        .height(8)
        .backgroundColor(Palette.control())
        .borderRadius(Radius.pill)
      Row()
        .width(this.levelWidth())
        .height(8)
        .backgroundColor(Palette.accent())
        .borderRadius(Radius.pill)
        .animation({ duration: Motion.quick, curve: Curve.EaseOut })
    }
    .width(96)
    .height(8)
  }

  @Builder
  silenceBlock() {
    Row({ space: Space.sm }) {
      Column({ space: Space.xxs }) {
        Text('停顿阈值')
          .fontSize(Type.body)
          .lineHeight(Leading.body)
          .fontColor(Palette.text1())
        Text('停顿超过这个时长就自动结束录音')
          .fontSize(Type.caption)
          .lineHeight(Leading.caption)
          .fontColor(Palette.text3())
      }
      .alignItems(HorizontalAlign.Start)
      .layoutWeight(1)

      Text(this.silenceLabel())
        .fontSize(Type.label)
        .lineHeight(Leading.label)
        .fontColor(Palette.text2())

      TextButton({
        label: '−',
        onTap: () => {
          this.shiftSilence(-0.5);
        }
      })
      TextButton({
        label: '+',
        onTap: () => {
          this.shiftSilence(0.5);
        }
      })
    }
    .width('100%')
    .padding({ left: Space.md, right: Space.md, top: Space.sm, bottom: Space.sm })
  }

  @Builder
  syncActionsBlock() {
    Column({ space: Space.xs }) {
      Row() {
        PrimaryButton({
          label: this.syncing ? '处理中…' : '上传本机偏好',
          usable: !this.syncing,
          onTap: () => {
            this.syncUpload();
          }
        })
      }
      .width('100%')

      Row({ space: Space.sm }) {
        TonalButton({
          label: '从云端恢复',
          symbol: 'refresh',
          usable: !this.syncing,
          onTap: () => {
            this.syncRestore();
          }
        })
        TextButton({
          label: '删除云端快照',
          symbol: 'trash',
          tone: 'danger',
          usable: !this.syncing,
          onTap: () => {
            this.syncDelete();
          }
        })
      }
      .width('100%')

      Text('冲突时以云端修订号（当前 ' + this.syncRevision.toString() + '）为准；口令与密钥留在系统密钥库，不上传。')
        .fontSize(Type.caption)
        .lineHeight(Leading.caption)
        .fontColor(Palette.text3())
        .width('100%')
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
    .padding({ left: Space.md, right: Space.md, top: Space.sm, bottom: Space.sm })
  }

  // ── Sheets ──────────────────────────────────────────────────────────────────

  @Builder
  asrSheet() {
    Column({ space: Space.sm }) {
      SheetTitle({
        title: '选择转写引擎',
        hint: '当前：' + Labels.provider(this.prefs.activeAsrProvider)
      })
      if (this.asrProviders.length === 0) {
        EmptyHint({
          symbol: 'localasr',
          title: '读不到供应商列表',
          hint: '检查网络后点右上角重新读取'
        })
      } else {
        Scroll() {
          Column() {
            Group() {
              ForEach(this.asrProviders, (descriptor: ProviderDescriptor, index: number) => {
                RowItem({
                  symbol: this.activeProvider(descriptor.providerType, true) ? 'check' : '',
                  accentSymbol: this.activeProvider(descriptor.providerType, true),
                  title: Labels.provider(descriptor.providerType),
                  subtitle: descriptor.providerType,
                  showChevron: false,
                  showDivider: index < this.asrProviders.length - 1,
                  onTap: () => {
                    this.activateAsr(descriptor.providerType);
                  }
                })
              }, (descriptor: ProviderDescriptor) => 'asr-' + descriptor.kind + '-' + descriptor.providerType)
            }
          }
          .width('100%')
        }
        .layoutWeight(1)
        .width('100%')
        .align(Alignment.Top)
        .scrollBar(BarState.Off)
        .edgeEffect(EdgeEffect.Spring)
      }
    }
    .width('100%')
    .height('100%')
    .alignItems(HorizontalAlign.Start)
    .padding({ left: Space.md, right: Space.md, top: Space.xs, bottom: Space.lg })
  }

  @Builder
  llmSheet() {
    Column({ space: Space.sm }) {
      SheetTitle({
        title: '选择润色模型',
        hint: '当前：' + Labels.provider(this.prefs.activeLlmProvider)
      })
      if (this.llmProviders.length === 0) {
        EmptyHint({
          symbol: 'style',
          title: '读不到供应商列表',
          hint: '检查网络后点右上角重新读取'
        })
      } else {
        Scroll() {
          Column() {
            Group() {
              ForEach(this.llmProviders, (descriptor: ProviderDescriptor, index: number) => {
                RowItem({
                  symbol: this.activeProvider(descriptor.providerType, false) ? 'check' : '',
                  accentSymbol: this.activeProvider(descriptor.providerType, false),
                  title: Labels.provider(descriptor.providerType),
                  subtitle: descriptor.providerType,
                  showChevron: false,
                  showDivider: index < this.llmProviders.length - 1,
                  onTap: () => {
                    this.activateLlm(descriptor.providerType);
                  }
                })
              }, (descriptor: ProviderDescriptor) => 'llm-' + descriptor.kind + '-' + descriptor.providerType)
            }
          }
          .width('100%')
        }
        .layoutWeight(1)
        .width('100%')
        .align(Alignment.Top)
        .scrollBar(BarState.Off)
        .edgeEffect(EdgeEffect.Spring)
      }
    }
    .width('100%')
    .height('100%')
    .alignItems(HorizontalAlign.Start)
    .padding({ left: Space.md, right: Space.md, top: Space.xs, bottom: Space.lg })
  }

  @Builder
  micSheet() {
    Column({ space: Space.sm }) {
      SheetTitle({
        title: '选择输入设备',
        hint: this.microphones.length.toString() + ' 个可用输入'
      })
      Scroll() {
        Column({ space: Space.sm }) {
          Segmented({
            items: this.micOptions(),
            selected: this.micIndex(),
            onSelect: (index: number) => {
              this.pickMicrophoneByIndex(index);
            }
          })
          if (this.microphones.length === 0) {
            Text('没有检测到独立的输入设备，系统默认即可')
              .fontSize(Type.caption)
              .lineHeight(Leading.caption)
              .fontColor(Palette.text3())
              .width('100%')
          }
          Row() {
            TextButton({
              label: '重新检测设备',
              symbol: 'refresh',
              onTap: () => {
                this.refreshMicrophones();
              }
            })
          }
          .width('100%')
        }
        .width('100%')
        .alignItems(HorizontalAlign.Start)
      }
      .layoutWeight(1)
      .width('100%')
      .align(Alignment.Top)
      .scrollBar(BarState.Off)
    }
    .width('100%')
    .height('100%')
    .alignItems(HorizontalAlign.Start)
    .padding({ left: Space.md, right: Space.md, top: Space.xs, bottom: Space.lg })
  }

  /** Which provider the sheet should mark as current. */
  private activeProvider(provider: string, asr: boolean): boolean {
    if (asr) return provider === this.prefs.activeAsrProvider;
    return provider === this.prefs.activeLlmProvider;
  }
}
