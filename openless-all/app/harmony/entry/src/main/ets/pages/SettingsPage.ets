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
  Tag,
  TextButton,
  TonalButton,
  notify
} from '../components/Kit';
import { Nav, NavItem } from '../components/Nav';
import { Api, CredentialStatus, NetworkState } from '../data/Api';
import {
  Channel,
  CloudSyncStatus,
  HistoryEntry,
  NativeReply,
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
/** Native names, as `translation_target_language` expects; empty = translation off. */
const LANG_IDS: string[] = ['', '简体中文', 'English', '日本語', '한국어'];
const LANG_LABELS: string[] = ['关闭翻译', '简体中文', 'English', '日本語', '한국어'];

/** Carries the preference document through the typed `UserPreferences` shape. */
interface ThemeCarrierPreferences {
  themeMode?: string;
}

interface ThemeCarrier {
  preferences: ThemeCarrierPreferences;
}

/**
 * Settings v2.
 *
 * Grouping matches what the Harmony build actually uses:
 *   大模型配置 / 外观 / 语言 / 权限与数据 / 云同步 / 工具 / 关于
 *
 * Everything writes through `update_setting_fields` with the pointer names the
 * shared preference document really has (`/themeMode`, `/translationTargetLanguage`,
 * `/pipelineMode`, ...), so no control is a placeholder: the appearance switch
 * pushes `setColorMode`, the model section validates the real channels, and the
 * language choice lands in the document the dictation pipeline reads.
 *
 * Wide windows show the configuration column next to the data column; compact
 * windows stack the same sections in reading order.
 */
@Component
export struct SettingsPage {
  @StorageLink('bp') bp: string = 'md';
  @StorageLink('backendRevision') revision: number = 0;
  @StorageLink('micLevel') micLevel: number = 0;

  @State private prefs: UserPreferences = {};
  @State private themeMode: string = 'system';
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
  @State private clearOpen: boolean = false;
  @State private micTesting: boolean = false;
  @State private micPermission: string = 'unknown';
  @State private logPath: string = '';
  @State private syncing: boolean = false;
  @State private clearing: boolean = false;
  @State private asrProviders: ProviderDescriptor[] = [];
  @State private llmProviders: ProviderDescriptor[] = [];
  @State private asrChannels: Channel[] = [];
  @State private llmChannels: Channel[] = [];
  @State private credentials: CredentialStatus = {};
  @State private checkState: string = '';
  @State private checkNote: string = '';
  @State private checking: boolean = false;

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
      this.themeMode = this.readThemeMode(snapshot);
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
      this.asrChannels = await Api.channels('asr');
      this.llmChannels = await Api.channels('llm');
      this.credentials = await Api.credentialStatus();
      await this.loadMicPermission();
      // Keep the window in step with the stored preference (the shell also applies
      // it on boot; re-applying here is idempotent and self-heals a stale window).
      this.applyCurrentAppearance();
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

  /**
   * `UserPreferences` in `data/Types` does not declare the theme field yet, so the
   * document is re-read through a local carrier instead of guessing the name:
   * the preference is `themeMode` (`system | light | dark`).
   */
  private readThemeMode(snapshot: SettingsSnapshot): string {
    try {
      const carrier: ThemeCarrier = JSON.parse(JSON.stringify(snapshot)) as ThemeCarrier;
      const mode: string | undefined = carrier.preferences.themeMode;
      if (mode === undefined || mode.length === 0) return 'system';
      return mode;
    } catch (error) {
      return 'system';
    }
  }

  private async loadMicPermission(): Promise<void> {
    try {
      const raw: string = await Api.raw('check_microphone_permission', '{}');
      const reply: NativeReply = JSON.parse(raw) as NativeReply;
      if (!reply.ok || reply.value === undefined || reply.value === null) {
        this.micPermission = 'unknown';
        return;
      }
      const serialized: string = JSON.stringify(reply.value);
      this.micPermission = serialized.split('"').join('');
    } catch (error) {
      this.micPermission = 'unknown';
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

  // ── Appearance ──────────────────────────────────────────────────────────────

  /**
   * Appearance is a real preference: persist `/themeMode`, then push it into the
   * application context so light/dark follows the user instead of the system.
   */
  private async applyTheme(theme: string): Promise<void> {
    if (this.themeMode === theme) return;
    const saved: boolean = await this.writeString('/themeMode', theme);
    if (!saved) {
      notify('外观切换失败');
      return;
    }
    this.themeMode = theme;
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
    Appearance.apply(context as common.UIAbilityContext, this.themeMode);
    return true;
  }

  // ── Models ──────────────────────────────────────────────────────────────────

  /**
   * Pipeline routing is two preferences: `multimodalPipelineEnabled` gates the
   * whole multimodal path, `pipelineMode` picks the configured mode. Writing the
   * mode first avoids briefly activating a stale multimodal config.
   */
  private async applyPipelineMode(mode: string): Promise<void> {
    this.busy = true;
    try {
      const snapshot: SettingsSnapshot = await Api.writeString('/pipelineMode', mode);
      this.prefs = snapshot.preferences;
      const enabled: SettingsSnapshot = await Api.writeBool('/multimodalPipelineEnabled', mode === 'multimodal');
      this.prefs = enabled.preferences;
      notify('处理管线已切换为' + Labels.pipeline(mode));
      this.error = '';
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
      this.credentials = await Api.credentialStatus();
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
      this.credentials = await Api.credentialStatus();
      this.llmOpen = false;
      notify('润色模型已切换为' + Labels.provider(provider));
      this.error = '';
    } catch (error) {
      this.error = (error as Error).message;
    } finally {
      this.busy = false;
    }
  }

  /** Channel that the active provider actually resolves to, for the self-check. */
  private channelFor(kind: string): string {
    const channels: Channel[] = kind === 'asr' ? this.asrChannels : this.llmChannels;
    if (channels.length === 0) return '';
    const active: string = kind === 'asr' ? (this.prefs.activeAsrProvider ?? '') : (this.prefs.activeLlmProvider ?? '');
    for (let index = 0; index < channels.length; index++) {
      if (channels[index].id === active) return channels[index].id;
    }
    for (let index = 0; index < channels.length; index++) {
      if (channels[index].providerType === active) return channels[index].id;
    }
    for (let index = 0; index < channels.length; index++) {
      if (channels[index].enabled) return channels[index].id;
    }
    return channels[0].id;
  }

  /**
   * Connectivity self-check: validate the channel the active provider resolves to,
   * record the result so the credentials page shows it, and report both kinds.
   */
  private async runSelfCheck(): Promise<void> {
    if (this.checking) return;
    const asrId: string = this.channelFor('asr');
    const llmId: string = this.channelFor('llm');
    if (asrId.length === 0 && llmId.length === 0) {
      this.checkState = 'fail';
      this.checkNote = '还没有渠道；先到「凭据与密钥」新建一个再自检';
      notify('没有可自检的渠道');
      return;
    }
    this.checking = true;
    this.checkState = 'running';
    this.checkNote = '正在对当前渠道各发一次最小请求…';
    const parts: string[] = [];
    let passed: number = 0;
    let total: number = 0;
    if (asrId.length > 0) {
      total = total + 1;
      const ok: boolean = await this.validateOne('asr', asrId, parts);
      if (ok) passed = passed + 1;
    }
    if (llmId.length > 0) {
      total = total + 1;
      const ok: boolean = await this.validateOne('llm', llmId, parts);
      if (ok) passed = passed + 1;
    }
    this.checking = false;
    this.checkNote = parts.join(' · ');
    if (passed === total) {
      this.checkState = 'ok';
      notify('连通正常 · ' + this.checkNote);
    } else if (passed > 0) {
      this.checkState = 'partial';
      notify('部分渠道未通过 · ' + this.checkNote);
    } else {
      this.checkState = 'fail';
      notify('渠道校验未通过，请检查密钥与接入点');
    }
    try {
      this.asrChannels = await Api.channels('asr');
      this.llmChannels = await Api.channels('llm');
    } catch (error) {
      console.warn('[OpenLess Settings] channel list not refreshed');
    }
  }

  private async validateOne(kind: string, channelId: string, parts: string[]): Promise<boolean> {
    const label: string = kind === 'asr' ? '转写 ' : '润色 ';
    const started: number = Date.now();
    try {
      const ok: boolean = await Api.validateChannel(kind, channelId);
      const latency: number = Date.now() - started;
      try {
        await Api.recordChannelTest(kind, channelId, ok, latency, ok ? '' : '校验未通过');
      } catch (error) {
        console.warn('[OpenLess Settings] channel test not recorded');
      }
      parts.push(ok ? label + latency.toString() + ' ms' : label + '失败');
      return ok;
    } catch (error) {
      parts.push(label + '失败');
      return false;
    }
  }

  // ── Language ────────────────────────────────────────────────────────────────

  /** `/translationTargetLanguage` takes the native language name; empty = off. */
  private async applyLanguage(name: string): Promise<void> {
    const saved: boolean = await this.writeString('/translationTargetLanguage', name);
    if (!saved) {
      notify('语言设置保存失败');
      return;
    }
    notify(name.length === 0 ? '已关闭翻译输出' : '翻译输出语言已改为' + name);
  }

  // ── Microphone and permissions ──────────────────────────────────────────────

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

  private async pickMicrophone(name: string): Promise<void> {
    this.micOpen = false;
    const saved: boolean = await this.writeString('/microphoneDeviceName', name);
    notify(saved ? (name.length === 0 ? '已改用系统默认麦克风' : '已切换麦克风：' + name) : '切换麦克风失败');
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

  private async requestMic(): Promise<void> {
    try {
      const result: string = await Api.requestMicrophone();
      this.micPermission = result;
      notify(result === 'granted' ? '麦克风权限已授权' : '未获得授权；可到系统设置里手动开启');
      this.error = '';
    } catch (error) {
      this.error = (error as Error).message;
      notify('请求麦克风权限失败');
    }
  }

  private async openMicSettings(): Promise<void> {
    try {
      await Api.openSystemMicrophoneSettings();
      notify('已打开系统的麦克风权限页');
    } catch (error) {
      this.error = (error as Error).message;
    }
  }

  private micPermissionLabel(): string {
    if (this.micPermission === 'granted') return '已授权';
    if (this.micPermission === 'denied') return '已拒绝';
    if (this.micPermission === 'indeterminate') return '未授权';
    return '未知';
  }

  private micPermissionHint(): string {
    if (this.micPermission === 'granted') return '已授权：听写、电平测试与输入法语音都可以直接使用。';
    if (this.micPermission === 'denied') return '已被拒绝：系统不会再弹授权框，只能到系统设置里手动开启。';
    return '首次听写时会弹系统授权框；也可以在这里主动请求，或直接跳系统设置。';
  }

  // ── Data ────────────────────────────────────────────────────────────────────

  private async clearAllHistory(): Promise<void> {
    if (this.clearing) return;
    this.clearing = true;
    try {
      await Api.clearHistory();
      this.clearOpen = false;
      this.historyCount = 0;
      notify('本机历史已清空');
      this.error = '';
      await this.reload();
    } catch (error) {
      this.error = (error as Error).message;
      notify('清空历史失败');
    } finally {
      this.clearing = false;
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
    for (let index = 0; index < THEME_IDS.length; index++) {
      if (THEME_IDS[index] === this.themeMode) return index;
    }
    return 0;
  }

  private pipelineIndex(): number {
    return this.prefs.pipelineMode === 'multimodal' ? 1 : 0;
  }

  private langIndex(): number {
    const current: string = this.prefs.translationTargetLanguage ?? '';
    for (let index = 0; index < LANG_IDS.length; index++) {
      if (LANG_IDS[index] === current) return index;
    }
    return 0;
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

  private channelSummary(): string {
    return '转写 ' + this.asrChannels.length.toString() + ' 个 · 润色 ' +
      this.llmChannels.length.toString() + ' 个';
  }

  private configuredSummary(): string {
    const asr: string = this.credentials.asrConfigured === true ? '转写已配置' : '转写未配置';
    const llm: string = this.credentials.llmConfigured === true ? '润色已配置' : '润色未配置';
    return asr + ' · ' + llm;
  }

  private checkTagText(): string {
    if (this.checking || this.checkState === 'running') return '检测中';
    if (this.checkState === 'ok') return '通过';
    if (this.checkState === 'partial') return '部分失败';
    if (this.checkState === 'fail') return '失败';
    return '未测试';
  }

  private checkTone(): string {
    if (this.checking || this.checkState === 'running') return 'accent';
    if (this.checkState === 'ok') return 'success';
    if (this.checkState === 'partial') return 'warning';
    if (this.checkState === 'fail') return 'danger';
    return 'neutral';
  }

  private checkDetail(): string {
    if (this.checkNote.length > 0) return this.checkNote;
    return '对当前转写与润色渠道各发一次最小请求，结果会回写到凭据页';
  }

  private levelWidth(): number {
    const level: number = Math.min(1, Math.max(0, this.micLevel));
    return Math.round(Math.max(4, level * 96));
  }

  private choiceItems(kind: string): string[] {
    if (kind === 'theme') return THEME_LABELS;
    if (kind === 'pipeline') return PIPELINE_LABELS;
    return LANG_LABELS;
  }

  private choiceSelected(kind: string): number {
    if (kind === 'theme') return this.themeIndex();
    if (kind === 'pipeline') return this.pipelineIndex();
    return this.langIndex();
  }

  private applyChoice(kind: string, index: number): void {
    if (kind === 'theme') {
      this.applyTheme(THEME_IDS[index]);
      return;
    }
    if (kind === 'pipeline') {
      this.applyPipelineMode(PIPELINE_IDS[index]);
      return;
    }
    this.applyLanguage(LANG_IDS[index]);
  }

  private activeProvider(provider: string, asr: boolean): boolean {
    if (asr) return provider === this.prefs.activeAsrProvider;
    return provider === this.prefs.activeLlmProvider;
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
            this.modelSection()
            this.appearanceSection()
            this.languageSection()
          }
          .layoutWeight(1)
          .alignItems(HorizontalAlign.Start)

          Column({ space: Space.md }) {
            this.privacySection()
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
          this.modelSection()
          this.appearanceSection()
          this.languageSection()
          this.privacySection()
          this.cloudSection()
          this.toolsSection()
          this.aboutSection()
        }
        .width('100%')
      }
    }
  }

  // ── 大模型配置 ──────────────────────────────────────────────────────────────

  @Builder
  modelSection() {
    Column({ space: Space.xs }) {
      SectionTitle({ title: '大模型配置', hint: '转写、润色与渠道连通性' })
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

        this.choiceBlock('pipeline', '处理管线', '全模态把转写与润色合成一次调用，延迟更低；切换会同时开关多模态配置', true)

        SettingSwitch({
          label: '深度思考',
          hint: '更慢，但长句改写更稳',
          checked: this.prefs.llmThinkingEnabled === true,
          onToggle: (value: boolean) => {
            this.writeBool('/llmThinkingEnabled', value);
          }
        })

        this.connectivityBlock()

        RowItem({
          title: '渠道与模型',
          subtitle: '在凭据页新建渠道、拉取模型列表或直接填模型 ID',
          value: this.channelSummary(),
          showChevron: true,
          showDivider: true,
          onTap: () => Nav.go(Nav.CREDENTIALS)
        })
        RowItem({
          title: '凭据与密钥',
          subtitle: 'API Key / App ID 存在系统密钥库，不写进偏好文件',
          value: this.configuredSummary(),
          showChevron: true,
          showDivider: false,
          onTap: () => Nav.go(Nav.CREDENTIALS)
        })
      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
  }

  /** Self-check row: state Tag + one action, result also recorded in the backend. */
  @Builder
  connectivityBlock() {
    Column() {
      Row({ space: Space.sm }) {
        Column({ space: Space.xxs }) {
          Text('连通性自检')
            .fontSize(Type.body)
            .lineHeight(Leading.body)
            .fontColor(Palette.text1())
          Text(this.checkDetail())
            .fontSize(Type.caption)
            .lineHeight(Leading.caption)
            .fontColor(Palette.text3())
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)

        Tag({ text: this.checkTagText(), tone: this.checkTone() })
        TextButton({
          label: this.checking ? '检测中…' : '自检',
          usable: !this.checking,
          onTap: () => {
            this.runSelfCheck();
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

  // ── 外观 ────────────────────────────────────────────────────────────────────

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

  // ── 语言 ────────────────────────────────────────────────────────────────────

  @Builder
  languageSection() {
    Column({ space: Space.xs }) {
      SectionTitle({ title: '语言', hint: '识别由引擎决定，输出语言在这里指定' })
      Group() {
        RowItem({
          title: '识别语言',
          value: '自动判定',
          subtitle: '转写引擎按音频自动识别语种；本地模型在「模型设置」里选语种',
          showDivider: true
        })
        this.choiceBlock('lang', '翻译输出语言', '翻译模式与划词翻译都会输出这个语种', true)
        Column({ space: Space.xxs }) {
          Text('界面文案暂为中文；多语言资源与界面翻译会在后续版本接入。')
            .fontSize(Type.caption)
            .lineHeight(Leading.caption)
            .fontColor(Palette.text3())
            .width('100%')
        }
        .width('100%')
        .alignItems(HorizontalAlign.Start)
        .padding({ left: Space.md, right: Space.md, top: Space.sm, bottom: Space.sm })
      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
  }

  // ── 权限与数据 ──────────────────────────────────────────────────────────────

  @Builder
  privacySection() {
    Column({ space: Space.xs }) {
      SectionTitle({ title: '权限与数据', hint: '麦克风、剪贴板与本机数据' })
      Group() {
        RowItem({
          title: '麦克风权限',
          value: this.micPermissionLabel(),
          showDivider: true,
          onTap: () => {
            this.requestMic();
          }
        })

        Column({ space: Space.xs }) {
          Row({ space: Space.sm }) {
            TonalButton({
              label: '请求授权',
              symbol: 'mic',
              onTap: () => {
                this.requestMic();
              }
            })
            TextButton({
              label: '系统设置',
              symbol: 'settings',
              onTap: () => {
                this.openMicSettings();
              }
            })
          }
          .width('100%')

          Text(this.micPermissionHint())
            .fontSize(Type.caption)
            .lineHeight(Leading.caption)
            .fontColor(Palette.text3())
            .width('100%')
        }
        .width('100%')
        .alignItems(HorizontalAlign.Start)
        .padding({ left: Space.md, right: Space.md, top: Space.sm, bottom: Space.sm })

        Divider()
          .color(Palette.line())
          .strokeWidth(0.5)
          .margin({ left: Space.md })

        RowItem({
          symbol: 'mic',
          title: '输入设备',
          subtitle: this.microphones.length.toString() + ' 个可用输入',
          value: this.microphoneValue(),
          showChevron: true,
          showDivider: true,
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

        RowItem({
          title: '剪贴板',
          value: '只写不读',
          subtitle: '不申请 READ_PASTEBOARD：本机只写入复制结果，读不到其它应用复制的内容',
          showDivider: true
        })
        SettingSwitch({
          label: '保留录音以便重新转写',
          hint: '开启后历史条目可以从设备上的录音重新识别',
          checked: this.prefs.recordAudioForDebug === true,
          onToggle: (value: boolean) => {
            this.writeBool('/recordAudioForDebug', value);
          }
        })
        RowItem({
          symbol: 'history',
          title: '历史记录',
          value: this.historyCount.toString() + ' 条',
          showChevron: true,
          showDivider: true,
          onTap: () => Nav.go(Nav.HISTORY)
        })
        RowItem({
          title: '清空本机历史',
          subtitle: '删除全部转录、录音与统计，不可恢复',
          showChevron: true,
          showDivider: true,
          onTap: () => {
            this.clearOpen = true;
          }
        })

        Row()
          .width(0)
          .height(0)
          .bindSheet($$this.clearOpen, this.clearSheet, {
            height: SheetSize.MEDIUM,
            dragBar: true,
            showClose: true,
            onDisappear: () => {
              this.clearOpen = false;
            }
          })

        RowItem({
          title: '导出诊断日志',
          subtitle: this.logPath.length > 0 ? this.logPath : '把最近的错误记录写成 JSON 文件，便于反馈问题',
          value: this.logPath.length > 0 ? '已导出' : '',
          showChevron: this.logPath.length === 0,
          showDivider: true,
          onTap: () => {
            this.exportLog();
          }
        })
        RowItem({
          title: '云端快照删除',
          subtitle: '删除入口在「云同步」组；删除只移除云端加密快照，本机偏好与历史保留',
          showDivider: false
        })
      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
  }

  /** Live input level + monitor toggle, next to the microphone permission. */
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

  // ── 云同步 ──────────────────────────────────────────────────────────────────

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

  // ── 工具 ────────────────────────────────────────────────────────────────────

  @Builder
  toolsSection() {
    Column({ space: Space.xs }) {
      SectionTitle({ title: '工具', hint: '选取助手、翻译、词典、纠错、问答、凭据与模型设置' })
      Group() {
        ForEach(Nav.tools(), (item: NavItem, index: number) => {
          RowItem({
            symbol: item.id,
            title: item.label,
            showChevron: true,
            showDivider: index < Nav.tools().length - 1,
            onTap: () => Nav.go(item.id)
          })
        }, (item: NavItem) => 'tool-' + item.id)
      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
  }

  // ── 关于 ────────────────────────────────────────────────────────────────────

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
          showDivider: false
        })
      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
  }

  // ── Shared blocks ───────────────────────────────────────────────────────────

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

  /** Destructive confirmation: clearing history is a separate, explicit step. */
  @Builder
  clearSheet() {
    Column({ space: Space.sm }) {
      SheetTitle({
        title: '清空本机历史',
        hint: '将删除全部 ' + this.historyCount.toString() + ' 条记录、对应录音与统计'
      })
      Text('这个操作不可撤销。云端加密快照不受影响；需要删除它请到「云同步」组。')
        .fontSize(Type.label)
        .lineHeight(Leading.label)
        .fontColor(Palette.text2())
        .width('100%')
      Row({ space: Space.sm }) {
        PrimaryButton({
          label: this.clearing ? '正在清空…' : '确认清空',
          destructive: true,
          usable: !this.clearing,
          onTap: () => {
            this.clearAllHistory();
          }
        })
        TextButton({
          label: '取消',
          tone: 'quiet',
          onTap: () => {
            this.clearOpen = false;
          }
        })
      }
      .width('100%')
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
    .padding({ left: Space.md, right: Space.md, top: Space.xs, bottom: Space.lg })
  }
}
