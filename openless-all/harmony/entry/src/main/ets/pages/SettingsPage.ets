import { LengthMetrics } from '@kit.ArkUI';
import { SettingsChoiceRow, SettingsNavRow } from '../components/settings/SettingsControls';
import { SettingsSection } from '../components/settings/SettingsSection';
import {
  SettingsActionRow,
  SettingsDangerRow,
  SettingsFootnoteRow,
  SettingsMeterRow,
  SettingsProfileRow
} from '../components/settings/SettingsRows';
import { common } from '@kit.AbilityKit';
import { Appearance } from '../common/Appearance';
import { Layout, Leading, Palette, Radius, Space, Type } from '../common/Theme';
import {
  EmptyHint,
  ErrorNote,
  Group,
  PageScaffold,
  PrimaryButton,
  RowItem,
  PinnedSearchBar,
  Segmented,
  SettingSwitch,
  SheetTitle,
  TextButton,
  notify
} from '../components/Kit';
import { Nav } from '../components/Nav';
import { Api, NetworkState, UpdateCheckResult } from '../data/Api';
import {
  CloudSyncStatus,
  HistoryEntry,
  NativeReply,
  PlatformCapabilities,
  SettingsSnapshot,
  StartupSnapshot,
  UserPreferences
} from '../data/Types';

const THEME_IDS: string[] = ['system', 'light', 'dark'];
const THEME_LABELS: string[] = ['跟随系统', '浅色', '深色'];
const UPDATE_CHANNELS: string[] = ['stable', 'beta'];
const UPDATE_CHANNEL_LABELS: string[] = ['正式版', 'Beta'];
/** Native names, as `translation_target_language` expects; empty = translation off. */
const LANG_IDS: string[] = ['', '简体中文', 'English', '日本語', '한국어'];
const LANG_LABELS: string[] = ['关闭翻译', '简体中文', 'English', '日本語', '한국어'];

/** Wide windows turn the group list into the left pane; this id means "everything". */
const SECTION_ALL: string = 'all';

/**
 * One inset-grouped section, in iOS reading order.
 *
 * `keywords` mirror the iOS `matches("…")` strings: the pinned search bar filters
 * whole sections by matching the group title and every row title inside it, which
 * is exactly what `SettingsView.matches` does with its keyword blobs.
 */
interface SettingsGroupSpec {
  id: string;
  title: string;
  symbol: string;
  keywords: string;
}

const GROUP_SPECS: SettingsGroupSpec[] = [
  {
    id: 'models',
    title: '大模型与凭据',
    symbol: 'sparkles',
    keywords: '大模型与凭据 大模型 凭据 密钥 渠道 引擎 模型 转写 文字模型 风格 翻译 入口 ' +
      'model credentials key channel engine style translation'
  },
  {
    id: 'account',
    title: '账户与云备份',
    symbol: 'person',
    keywords: '账户与云备份 账户 账号 登录 云备份 云端 同步 上传 恢复 删除 快照 网络 ' +
      'account profile cloud sync backup login'
  },
  {
    id: 'voice',
    title: '语音与模型',
    symbol: 'localasr',
    keywords: '语音与模型 语音 识别 识别方式 端侧模型 麦克风 麦克风电平 输入设备 电平 测试 ' +
      'speech recognition local model microphone device level'
  },
  {
    id: 'keyboard',
    title: '键盘与输入',
    symbol: 'square_grid_2x2',
    keywords: '键盘与输入 键盘 键盘扩展 输入 输入法 输入法接入 打字 剪贴板 keyboard input ime typing clipboard'
  },
  {
    id: 'appearance',
    title: '外观与语言',
    symbol: 'style',
    keywords: '外观与语言 外观 颜色模式 颜色 模式 主题 深色 浅色 跟随系统 语言 翻译输出语言 界面语言 ' +
      'appearance theme color language translation'
  },
  {
    id: 'privacy',
    title: '权限与数据',
    symbol: 'settings',
    keywords: '权限与数据 权限 数据 隐私 麦克风权限 历史 历史记录 清空 清空本机历史 录音 保留录音 ' +
      'permission privacy data history clear'
  },
  {
    id: 'help',
    title: '帮助与关于',
    symbol: 'list',
    keywords: '帮助与关于 帮助 帮助中心 关于 版本 版本号 更新 检查更新 渠道 Beta 正式版 许可 开源 诊断 日志 导出诊断日志 平台 契约 离线听写 ' +
      'help about version update channel release license attribution log diagnostics platform contract'
  }
];

/** Left-pane rows on wide windows: the whole list, or one group. */
interface SettingsNavEntry {
  id: string;
  title: string;
  symbol: string;
}

/** Carries the preference document through the typed `UserPreferences` shape. */
interface ThemeCarrierPreferences {
  themeMode?: string;
}

interface ThemeCarrier {
  preferences: ThemeCarrierPreferences;
}

/**
 * Settings, ported row by row from `ios/App/SettingsView.swift`.
 *
 * The page is one inset-grouped list in iOS order: 大模型与凭据 (the iOS
 * "风格与翻译" entry position, and the only way into model configuration on
 * Harmony) → 账户与云备份 (the iOS account section, with the
 * `AccountProfileHeader` avatar/status row) → 语音与模型 → 键盘与输入 →
 * 外观与语言 → 权限与数据 → 帮助与关于 → the centred version footnote.
 *
 * A pinned iOS `PinnedSearchBar` filters sections locally by group and row
 * titles, exactly like `SettingsView.matches`. Wide windows keep the two-pane
 * layout, but both panes use the same 44pt rows, 28pt icon tiles, 13pt section
 * captions and 18pt cards as the compact list.
 *
 * Every control still writes through the typed `Api` façade
 * (`update_setting_fields`, cloud sync, microphone monitor, history export);
 * nothing was dropped while restyling, and no row is a placeholder switch.
 */
@Component
export struct SettingsPage {
  @StorageLink('bp') bp: string = 'md';
  @StorageLink('phoneLayout') phoneLayout: boolean = true;
  @StorageLink('backendRevision') revision: number = 0;
  @StorageLink('micLevel') micLevel: number = 0;

  @State private prefs: UserPreferences = {};
  @State private themeMode: string = 'system';
  @State private microphones: string[] = [];
  @State private network: string = '检测中…';
  @State private syncStatus: string = '未登录';
  @State private syncAccount: string = '';
  @State private syncLoggedIn: boolean = false;
  @State private syncHasSnapshot: boolean = false;
  @State private syncCountsText: string = '';
  @State private syncLast: string = '';
  @State private syncNote: string = '';
  @State private syncRevision: number = 0;
  @State private loginBusy: boolean = false;
  @State private loginCode: string = '';
  @State private loginState: string = '';
  @State private capabilities: PlatformCapabilities = { platform: 'harmony' };
  @State private contract: string = '';
  @State private historyCount: number = 0;
  @State private error: string = '';
  @State private busy: boolean = false;
  /** Pinned search text: pure local filter over section titles and row titles. */
  @State private query: string = '';
  /** Wide-window group index selection: `SECTION_ALL` or one group id. */
  @State private section: string = SECTION_ALL;
  @State private micOpen: boolean = false;
  @State private clearOpen: boolean = false;
  @State private syncDeleteOpen: boolean = false;
  @State private infoOpen: boolean = false;
  @State private infoMode: string = 'help';
  @State private updateChannel: string = 'stable';
  @State private updateChecking: boolean = false;
  @State private updateNote: string = '';
  @State private updateReleaseUrl: string = '';
  @State private updateAvailable: boolean = false;
  @State private micTesting: boolean = false;
  @State private micPermission: string = 'unknown';
  @State private logPath: string = '';
  @State private syncing: boolean = false;
  @State private clearing: boolean = false;
  private viewActive: boolean = false;
  private loginGeneration: number = 0;
  private loginFlowId: string = '';
  private loginTimer: number = -1;
  private loginWake?: () => void;

  aboutToAppear(): void {
    this.viewActive = true;
    this.reload();
  }

  aboutToDisappear(): void {
    this.viewActive = false;
    this.cancelLogin();
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
      await this.loadMicPermission();
      await this.loadUpdateChannel();
      // Keep the window in step with the stored preference (the shell also applies
      // it on boot; re-applying here is idempotent and self-heals a stale window).
      this.applyCurrentAppearance();
      this.error = '';
    } catch (error) {
      this.error = (error as Error).message;
    }
  }

  private async loadUpdateChannel(): Promise<void> {
    try {
      const channel: string = await Api.getUpdateChannel();
      this.updateChannel = channel === 'beta' ? 'beta' : 'stable';
    } catch (error) {
      this.updateChannel = 'stable';
    }
  }

  private updateChannelIndex(): number {
    return this.updateChannel === 'beta' ? 1 : 0;
  }

  private async switchUpdateChannel(index: number): Promise<void> {
    if (index < 0 || index >= UPDATE_CHANNELS.length || this.updateChecking) return;
    const next: string = UPDATE_CHANNELS[index];
    if (next === this.updateChannel) return;
    try {
      const saved: string = await Api.setUpdateChannel(next);
      this.updateChannel = saved === 'beta' ? 'beta' : 'stable';
      this.updateNote = '';
      this.updateReleaseUrl = '';
      this.updateAvailable = false;
      notify('更新渠道已切换为' + (this.updateChannel === 'beta' ? ' Beta' : '正式版'));
    } catch (error) {
      notify('切换更新渠道失败');
    }
  }

  private currentAppVersion(): string {
    return AppStorage.get<string>('appVersion') ?? '';
  }

  private async checkForUpdates(): Promise<void> {
    if (this.updateChecking) return;
    this.updateChecking = true;
    this.updateNote = '正在检查更新…';
    try {
      const result: UpdateCheckResult = await Api.checkUpdate(this.currentAppVersion(), this.updateChannel);
      this.updateAvailable = result.hasUpdate === true;
      this.updateReleaseUrl = result.releaseUrl ?? '';
      if (this.updateAvailable) {
        this.updateNote = '发现新版本 ' + (result.tagName.length > 0 ? result.tagName : result.latestVersion) + '（点按前往下载）';
        notify('发现新版本 ' + result.latestVersion);
      } else {
        this.updateNote = '当前已是最新（' + (result.latestVersion.length > 0 ? result.latestVersion : this.currentAppVersion()) + '）';
        notify('当前已是最新版本');
      }
    } catch (error) {
      this.updateAvailable = false;
      this.updateNote = '检查失败：' + (error as Error).message;
    } finally {
      this.updateChecking = false;
    }
  }

  private async openUpdateDownload(): Promise<void> {
    const url: string = this.updateReleaseUrl.length > 0
      ? this.updateReleaseUrl
      : 'https://github.com/Open-Less/openless/releases';
    try {
      await Api.openExternalUrl(url);
    } catch (error) {
      notify('无法打开浏览器');
    }
  }

  private openInfoSheet(mode: string): void {
    this.infoMode = mode;
    this.infoOpen = true;
  }

  private async loadSyncStatus(): Promise<void> {
    try {
      const auth = await Api.marketplaceAuthStatus();
      this.syncLoggedIn = auth.signedIn === true;
      if (!this.syncLoggedIn) {
        this.syncAccount = '';
        this.syncStatus = '未登录';
        this.syncHasSnapshot = false;
        this.syncCountsText = '';
        this.syncLast = '';
        this.syncRevision = 0;
        return;
      }
      const sync: CloudSyncStatus = await Api.cloudSyncStatus();
      this.syncRevision = sync.revision ?? 0;
      this.syncHasSnapshot = sync.hasSnapshot === true;
      this.syncLast = sync.updatedAt !== undefined && sync.updatedAt !== null ? sync.updatedAt : '';
      if (this.syncHasSnapshot && sync.counts !== undefined) {
        const dict: number = sync.counts.dictionary ?? 0;
        const rules: number = sync.counts.corrections ?? 0;
        const packs: number = sync.counts.stylePacks ?? 0;
        this.syncCountsText = '词典 ' + dict.toString() + ' · 纠错 ' + rules.toString() + ' · 风格 ' + packs.toString();
      } else {
        this.syncCountsText = '';
      }
      this.syncStatus = this.syncHasSnapshot ? '云端快照可用' : '已登录，尚无云端快照';
    } catch (error) {
      this.syncHasSnapshot = false;
      this.syncCountsText = '';
      this.syncLast = '';
      this.syncRevision = 0;
      this.syncStatus = this.syncLoggedIn ? '云端状态暂不可用' : '未登录';
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

  private applyThemeIndex(index: number): void {
    if (index < 0 || index >= THEME_IDS.length) return;
    this.applyTheme(THEME_IDS[index]);
  }

  private applyLanguageIndex(index: number): void {
    if (index < 0 || index >= LANG_IDS.length) return;
    this.applyLanguage(LANG_IDS[index]);
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

  // ── Cloud sync & GitHub account ──────────────────────────────────────────────

  private waitLogin(ms: number): Promise<void> {
    return new Promise<void>((resolve) => {
      this.loginWake = resolve;
      this.loginTimer = setTimeout(() => {
        this.loginTimer = -1;
        this.loginWake = undefined;
        resolve();
      }, ms);
    });
  }

  private cancelLogin(): void {
    this.loginGeneration++;
    if (this.loginTimer >= 0) clearTimeout(this.loginTimer);
    this.loginTimer = -1;
    const wake = this.loginWake;
    this.loginWake = undefined;
    if (wake !== undefined) wake();
    const flowId: string = this.loginFlowId;
    this.loginFlowId = '';
    this.loginBusy = false;
    this.loginCode = '';
    this.loginState = '';
    if (flowId.length > 0) Api.githubDeviceFlowCancel(flowId).catch(() => {});
  }

  private async startLogin(): Promise<void> {
    if (this.loginBusy || this.syncing || !this.viewActive) return;
    const generation: number = ++this.loginGeneration;
    this.loginBusy = true;
    this.loginState = '正在准备 GitHub 登录…';
    try {
      const flow = await Api.githubDeviceFlowStart();
      if (!this.viewActive || generation !== this.loginGeneration) {
        Api.githubDeviceFlowCancel(flow.flowId).catch(() => {});
        return;
      }
      this.loginFlowId = flow.flowId;
      this.loginCode = flow.userCode;
      this.loginState = '等待浏览器确认授权…';
      try { await Api.copyText(flow.userCode); } catch (error) { notify('请手动复制配对码'); }
      if (!this.viewActive || generation !== this.loginGeneration) return;
      await Api.openExternalUrl(flow.verificationUri);
      if (!this.viewActive || generation !== this.loginGeneration) return;
      const expiresSecs: number = flow.expiresInSecs ?? flow.expiresIn ?? 900;
      const intervalSecs: number = flow.intervalSecs ?? flow.interval ?? 5;
      const expiresAt: number = Date.now() + Math.max(1, expiresSecs) * 1000;
      let intervalMs: number = Math.max(2, intervalSecs) * 1000;
      while (Date.now() < expiresAt) {
        await this.waitLogin(Math.min(intervalMs, Math.max(1, expiresAt - Date.now())));
        if (!this.viewActive || generation !== this.loginGeneration) return;
        if (Date.now() >= expiresAt) break;
        const poll = await Api.githubDeviceFlowPoll(flow.flowId);
        if (!this.viewActive || generation !== this.loginGeneration) return;
        if (poll.kind === 'authorized') {
          this.syncLoggedIn = true;
          this.syncAccount = poll.login ?? 'GitHub';
          this.loginCode = '';
          this.loginState = '';
          this.loginFlowId = '';
          this.loginBusy = false;
          notify('已登录 GitHub');
          await this.loadSyncStatus();
          return;
        }
        if (poll.kind === 'slowDown') intervalMs += 5000;
        if (poll.kind === 'error') throw new Error(poll.message ?? '授权未完成，请重试');
      }
      throw new Error('登录已过期，请重试');
    } catch (error) {
      if (this.viewActive && generation === this.loginGeneration) {
        this.loginCode = '';
        this.loginState = '';
        this.syncNote = '登录失败：' + (error as Error).message;
        notify('登录失败：' + (error as Error).message);
      }
    } finally {
      if (generation === this.loginGeneration) {
        const flowId: string = this.loginFlowId;
        this.loginFlowId = '';
        this.loginBusy = false;
        if (flowId.length > 0) Api.githubDeviceFlowCancel(flowId).catch(() => {});
      }
    }
  }

  private async logoutAccount(): Promise<void> {
    if (this.loginBusy || this.syncing) return;
    this.loginBusy = true;
    try {
      await Api.marketplaceLogout();
      if (!this.viewActive) return;
      this.syncLoggedIn = false;
      this.syncAccount = '';
      this.syncHasSnapshot = false;
      this.syncCountsText = '';
      this.syncLast = '';
      this.syncRevision = 0;
      this.syncStatus = '未登录';
      this.syncNote = '已退出账户，本机词典、历史与设置已保留';
      notify('已退出 GitHub');
    } catch (error) {
      if (this.viewActive) {
        this.syncNote = '退出失败：' + (error as Error).message;
        notify('退出失败');
      }
    } finally {
      this.loginBusy = false;
    }
  }

  private async copyLoginCode(): Promise<void> {
    if (this.loginCode.length === 0) return;
    try {
      await Api.copyText(this.loginCode);
      notify('已复制登录代码');
    } catch (error) {
      notify('复制失败');
    }
  }

  private async syncUpload(): Promise<void> {
    if (!this.syncLoggedIn || this.syncing) return;
    this.syncing = true;
    this.syncNote = '正在上传…';
    try {
      // CloudSyncUiPreferences uses deny_unknown_fields (only optional locale / fontScale).
      await Api.cloudSyncUpload(this.syncRevision, '{}');
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
    if (!this.syncLoggedIn || this.syncing) return;
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
    if (!this.syncLoggedIn || this.syncing) return;
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
      this.syncDeleteOpen = false;
    }
  }

  // ── Derived labels ──────────────────────────────────────────────────────────

  private isWide(): boolean {
    return !this.phoneLayout && Layout.isWide(this.bp);
  }

  /** iOS prints `"OpenLess " + CFBundleShortVersionString`; same shape here. */
  private versionLabel(): string {
    const raw: string | undefined = AppStorage.get<string>('appVersion');
    if (raw === undefined || raw.length === 0) return 'OpenLess';
    return 'OpenLess ' + raw;
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

  /** `AccountProfileHeader` equivalent: the login name, or the sign-in prompt. */
  private profileTitle(): string {
    if (this.syncLoggedIn) {
      return this.syncAccount.length > 0 ? this.syncAccount : 'GitHub 账户';
    }
    return '登录 GitHub 账户';
  }

  private profileStatus(): string {
    if (this.loginState.length > 0) return this.loginState;
    if (this.syncLoggedIn) {
      const base: string = this.syncStatus + ' · ' + this.network;
      return this.syncCountsText.length > 0 ? base + ' · ' + this.syncCountsText : base;
    }
    return '未登录 · 登录后可同步词典、纠错规则与风格包';
  }

  private asrValue(): string {
    return '自动识别';
  }

  private asrHint(): string {
    if (this.capabilities.supportsLocalAsr === true) {
      return '转写引擎按音频自动判定语种；端侧模型可在「端侧模型」里配置';
    }
    return '当前引擎按音频自动识别语种；此设备暂未开放端侧识别';
  }

  private lastSyncValue(): string {
    return this.syncLast.length > 0 ? this.syncLast : '—';
  }

  private syncFooter(): string {
    return '云端修订 ' + this.syncRevision.toString() +
      ' · 口令与密钥留在系统密钥库，不上传；删除只移除云端加密快照，本机偏好与历史保留。';
  }

  // ── Search and grouping (local only) ────────────────────────────────────────

  private needle(): string {
    return this.query.trim().toLowerCase();
  }

  /**
   * Groups to render: everything (or the wide-pane selection) while the search
   * field is empty, and only matching groups while it is not.
   */
  private visibleIds(): string[] {
    const needle: string = this.needle();
    const out: string[] = [];
    for (let index = 0; index < GROUP_SPECS.length; index++) {
      const spec: SettingsGroupSpec = GROUP_SPECS[index];
      if (needle.length === 0) {
        // The left-pane selection only narrows the wide layout; a compact window
        // always reads the full list, even after a resize from lg/xl.
        if (!this.isWide() || this.section === SECTION_ALL || this.section === spec.id) out.push(spec.id);
      } else if (spec.keywords.toLowerCase().indexOf(needle) >= 0) {
        out.push(spec.id);
      }
    }
    return out;
  }

  private shows(id: string): boolean {
    return this.visibleIds().indexOf(id) >= 0;
  }

  private versionVisible(): boolean {
    const needle: string = this.needle();
    if (needle.length === 0) return true;
    return '版本 版本号 openless version'.indexOf(needle) >= 0;
  }

  private nothingMatched(): boolean {
    return this.visibleIds().length === 0 && !this.versionVisible();
  }

  private navEntries(): SettingsNavEntry[] {
    const out: SettingsNavEntry[] = [];
    const needle: string = this.needle();
    if (needle.length === 0) {
      out.push({ id: SECTION_ALL, title: '全部设置', symbol: 'settings' });
    }
    for (let index = 0; index < GROUP_SPECS.length; index++) {
      const spec: SettingsGroupSpec = GROUP_SPECS[index];
      if (this.shows(spec.id)) {
        out.push({ id: spec.id, title: spec.title, symbol: spec.symbol });
      }
    }
    return out;
  }

  // ── View ────────────────────────────────────────────────────────────────────

  build() {
    PageScaffold({
      title: '设置',
      scrollable: false,
      gap: Space.sm
    }) {
      Column({ space: Space.sm }) {
        this.searchBar()

        if (this.error.length > 0) {
          ErrorNote({
            message: this.error,
            onRetry: () => {
              this.reload();
            }
          })
        }

        if (this.isWide()) {
          this.wideBody()
        } else {
          this.compactBody()
        }

        this.sheets()
      }
      .width('100%')
      .layoutWeight(1)
      .alignItems(HorizontalAlign.Start)
    }
  }

  /** iOS `PinnedSearchBar`: 搜索设置, pinned above the list, hairline underneath. */
  @Builder
  searchBar() {
    PinnedSearchBar({
      placeholder: '搜索设置',
      value: this.query,
      inset: false,
      onInput: (value: string) => {
        this.query = value;
      }
    })
  }

  /** Compact: one scrolling inset-grouped list. */
  @Builder
  compactBody() {
    Scroll() {
      Column({ space: Space.lg }) {
        this.groups()
      }
      .width('100%')
      .padding({ bottom: Layout.dockClearance() })
      .alignItems(HorizontalAlign.Start)
    }
    .width('100%')
    .layoutWeight(1)
    .align(Alignment.Top)
    .scrollBar(BarState.Off)
    .edgeEffect(EdgeEffect.Spring)
  }

  /** Wide: group index on the left, the same inset-grouped list on the right. */
  @Builder
  wideBody() {
    Row({ space: Space.lg }) {
      Column({ space: Space.xs }) {
        Text('分组')
          .fontSize(Type.caption)
          .lineHeight(Leading.caption)
          .fontColor(Palette.text2())
          .width('100%')
          .padding({ left: 6 })

        Group() {
          ForEach(this.navEntries(), (entry: SettingsNavEntry, index: number) => {
            SettingsNavRow({
              title: entry.title,
              symbol: entry.symbol,
              selected: this.section === entry.id,
              showDivider: index < this.navEntries().length - 1,
              onTap: () => {
                this.section = entry.id;
              }
            })
          }, (entry: SettingsNavEntry) => 'nav-' + entry.id)
        }
      }
      .width(240)
      .alignItems(HorizontalAlign.Start)

      Scroll() {
        Column({ space: Space.lg }) {
          this.groups()
        }
        .width('100%')
        .padding({ bottom: Layout.dockClearance() })
        .alignItems(HorizontalAlign.Start)
      }
      .layoutWeight(1)
      .align(Alignment.Top)
      .scrollBar(BarState.Off)
      .edgeEffect(EdgeEffect.Spring)
    }
    .width('100%')
    .layoutWeight(1)
    .alignItems(VerticalAlign.Top)
  }

  /** The whole list in iOS order; the version footnote closes it. */
  @Builder
  groups() {
    if (this.shows('models')) {
      this.modelsGroup()
    }
    if (this.shows('account')) {
      this.accountGroup()
    }
    if (this.shows('voice')) {
      this.voiceGroup()
    }
    if (this.shows('keyboard')) {
      this.keyboardGroup()
    }
    if (this.shows('appearance')) {
      this.appearanceGroup()
    }
    if (this.shows('privacy')) {
      this.privacyGroup()
    }
    if (this.shows('help')) {
      this.helpGroup()
    }
    if (this.versionVisible()) {
      this.versionGroup()
    }
    if (this.nothingMatched()) {
      Group() {
        EmptyHint({
          symbol: 'search',
          title: '没有匹配的设置',
          hint: '换个关键词，例如「云备份」「麦克风」「外观」'
        })
      }
    }
  }

  /**
   * The one entry to model configuration, sitting exactly where iOS puts the
   * "风格与翻译" entry (SettingsView.swift:88-98): one sparkles row that owns the
   * whole page of engines, models, channels and keys.
   */
  @Builder
  modelsGroup() {
    SettingsSection({
      title: '大模型与凭据',
      footer: '引擎、文字模型、服务渠道与密钥都集中在这一页。'
    }) {
      RowItem({
        symbol: 'sparkles',
        title: '大模型配置',
        subtitle: '转写引擎、文字模型、服务渠道与密钥',
        showChevron: true,
        showDivider: false,
        onTap: () => {
          Nav.go(Nav.CREDENTIALS);
        }
      })
    }
  }

  /** iOS account section: the `AccountProfileHeader` row, then the cloud actions. */
  @Builder
  accountGroup() {
    SettingsSection({ title: '账户与云备份', footer: this.syncFooter() }) {
      SettingsProfileRow({
        title: this.profileTitle(),
        status: this.profileStatus(),
        showDivider: true,
        onTap: () => {
          if (!this.syncLoggedIn && !this.loginBusy) {
            this.startLogin();
          }
        }
      })

      if (this.loginCode.length > 0) {
        RowItem({
          symbol: 'copy',
          title: '登录代码 · ' + this.loginCode,
          subtitle: '在浏览器中输入该代码完成授权，点按可再次复制',
          value: '复制',
          valueAccent: true,
          showDivider: true,
          onTap: () => {
            this.copyLoginCode();
          }
        })
      }

      if (!this.syncLoggedIn) {
        SettingsActionRow({
          label: this.loginBusy ? '取消登录' : '使用 GitHub 登录',
          tone: this.loginBusy ? 'quiet' : 'accent',
          usable: !this.syncing,
          showDivider: true,
          onTap: () => {
            if (this.loginBusy) this.cancelLogin();
            else this.startLogin();
          }
        })
      }

      RowItem({
        title: '网络',
        value: this.network,
        showDivider: true
      })
      RowItem({
        title: '上次同步',
        value: this.lastSyncValue(),
        subtitle: this.syncNote,
        showDivider: this.syncLoggedIn
      })

      if (this.syncLoggedIn) {
        SettingsActionRow({
          label: this.syncing ? '处理中…' : '上传本机偏好',
          usable: !this.syncing && !this.loginBusy,
          showDivider: true,
          onTap: () => {
            this.syncUpload();
          }
        })
        SettingsActionRow({
          label: '从云端恢复',
          usable: !this.syncing && !this.loginBusy && this.syncHasSnapshot,
          showDivider: true,
          onTap: () => {
            this.syncRestore();
          }
        })
        SettingsDangerRow({
          label: '删除云端快照',
          usable: !this.syncing && !this.loginBusy && this.syncHasSnapshot,
          showDivider: true,
          onTap: () => {
            this.syncDeleteOpen = true;
          }
        })
        SettingsDangerRow({
          label: '退出 GitHub 账户',
          usable: !this.syncing && !this.loginBusy,
          showDivider: false,
          onTap: () => {
            this.logoutAccount();
          }
        })
      }
    }
  }

  /** iOS 语音识别 section: recognition mode, the on-device model, the microphone. */
  @Builder
  voiceGroup() {
    SettingsSection({
      title: '语音与模型',
      footer: '识别由引擎决定；录音只在你主动按下麦克风时采集。'
    }) {
      RowItem({
        symbol: 'localasr',
        title: '识别方式',
        value: this.asrValue(),
        subtitle: this.asrHint(),
        showDivider: true
      })
      RowItem({
        symbol: 'square_grid_2x2',
        title: '端侧模型',
        value: this.capabilities.supportsLocalAsr === true ? '可配置' : '未开放',
        subtitle: '设备上的离线转写模型与下载管理',
        showChevron: true,
        showDivider: true,
        onTap: () => {
          Nav.go(Nav.LOCALASR);
        }
      })
      RowItem({
        symbol: 'mic',
        title: '输入设备',
        value: this.microphoneValue(),
        valueAccent: true,
        subtitle: this.microphones.length.toString() + ' 个可用输入',
        showChevron: true,
        showDivider: true,
        onTap: () => {
          this.micOpen = true;
        }
      })
      SettingsMeterRow({
        title: '麦克风电平',
        hint: this.micTesting ? '正在采集，说一句话看条形' : '测一下，确认设备真的在收音',
        label: this.micTesting ? '停止' : '测试',
        level: this.micLevel,
        testing: this.micTesting,
        showDivider: false,
        onTap: () => {
          this.toggleMicTest();
        }
      })
    }
  }

  /** HarmonyOS ships its own native InputMethodExtensionAbility (`OpenLessInputMethod`). */
  @Builder
  keyboardGroup() {
    SettingsSection({
      title: '键盘与输入',
      footer: '在「系统设置 → 系统和更新 → 语言和输入法」中启用 OpenLess 输入法；键盘布局、按键音与字号可在输入法工具栏内随时调整。'
    }) {
      RowItem({
        symbol: 'square_grid_2x2',
        title: 'OpenLess 鸿蒙输入法',
        value: this.capabilities.supportsImeInput === true ? '已内置' : '未接入',
        subtitle: '内置 Rime 中文拼音与语音输入，长按空格或点麦克风即可在其他应用内直接落字',
        showDivider: true
      })
      RowItem({
        symbol: 'mic',
        title: '跨沙箱语音配置',
        value: '自动同步',
        subtitle: '主应用内保存的大模型密钥、润色模式与翻译目标会自动同步给输入法扩展使用',
        showDivider: true
      })
      RowItem({
        symbol: 'copy',
        title: '剪贴板',
        value: '按需粘贴',
        subtitle: '不默认申请后台读取剪贴板；主应用只写不读，输入法内可通过系统粘贴控件主动粘贴',
        showDivider: false
      })
    }
  }

  /** iOS 风格与翻译的第二行 (界面语言与外观), as HarmonyOS can actually offer it. */
  @Builder
  appearanceGroup() {
    SettingsSection({
      title: '外观与语言',
      footer: '跟随系统时随浅色/深色模式自动切换；界面文案暂为中文。'
    }) {
      SettingsChoiceRow({
        title: '颜色模式',
        hint: '立即应用到主界面与输入法面板',
        items: THEME_LABELS,
        selected: this.themeIndex(),
        showDivider: true,
        onSelect: (index: number) => {
          this.applyThemeIndex(index);
        }
      })
      SettingsChoiceRow({
        title: '翻译输出语言',
        hint: '翻译模式与划词翻译都会输出这个语种',
        items: LANG_LABELS,
        selected: this.langIndex(),
        showDivider: true,
        onSelect: (index: number) => {
          this.applyLanguageIndex(index);
        }
      })
      RowItem({
        title: '界面语言',
        value: '简体中文',
        subtitle: '多语言资源与界面翻译会在后续版本接入',
        showDivider: false
      })
    }
  }

  /** iOS 隐私与数据 + 系统录音: permissions first, then the local data controls. */
  @Builder
  privacyGroup() {
    SettingsSection({
      title: '权限与数据',
      footer: this.busy ? '正在保存…' : '历史与录音只留在本机；清空后不可恢复。'
    }) {
      RowItem({
        symbol: 'mic',
        title: '麦克风权限',
        value: this.micPermissionLabel(),
        subtitle: this.micPermissionHint(),
        showDivider: true,
        onTap: () => {
          this.requestMic();
        }
      })
      SettingsActionRow({
        label: '请求麦克风授权',
        showDivider: true,
        onTap: () => {
          this.requestMic();
        }
      })
      SettingsActionRow({
        label: '打开系统麦克风设置',
        showDivider: true,
        onTap: () => {
          this.openMicSettings();
        }
      })
      SettingSwitch({
        label: '保留录音以便重新转写',
        hint: '开启后历史条目可以从设备上的录音重新识别',
        checked: this.prefs.recordAudioForDebug === true,
        showDivider: true,
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
        onTap: () => {
          Nav.go(Nav.HISTORY);
        }
      })
      SettingsDangerRow({
        label: '清空本机历史',
        showDivider: false,
        onTap: () => {
          this.clearOpen = true;
        }
      })
    }
  }

  /** iOS 帮助与关于, including update checks, usage help and open-source attributions. */
  @Builder
  helpGroup() {
    SettingsSection({
      title: '帮助与关于',
      footer: '诊断日志仅包含最近会话记录，不会自动上传；OpenLess 2.0 依据 AGPL-3.0-only 开源。'
    }) {
      SettingsChoiceRow({
        title: '更新渠道',
        hint: '正式版更稳定；Beta 渠道可抢先体验新功能',
        items: UPDATE_CHANNEL_LABELS,
        selected: this.updateChannelIndex(),
        showDivider: true,
        onSelect: (index: number) => {
          this.switchUpdateChannel(index);
        }
      })
      RowItem({
        symbol: 'refresh',
        title: '检查更新',
        value: this.updateChecking ? '检查中…' : (this.updateAvailable ? '有新版本' : '检查'),
        valueAccent: this.updateAvailable,
        subtitle: this.updateNote.length > 0 ? this.updateNote : '检查 GitHub Releases 最新版本',
        showChevron: true,
        showDivider: true,
        onTap: () => {
          if (this.updateAvailable && this.updateReleaseUrl.length > 0) {
            this.openUpdateDownload();
          } else {
            this.checkForUpdates();
          }
        }
      })
      RowItem({
        symbol: 'list',
        title: '帮助中心',
        subtitle: '快速上手录音授权、鸿蒙输入法与云端/全模态配置',
        showChevron: true,
        showDivider: true,
        onTap: () => {
          this.openInfoSheet('help');
        }
      })
      RowItem({
        symbol: 'list',
        title: '开源许可与组件署名',
        subtitle: 'AGPL-3.0-only · Rime · Open Runde · Rare UI',
        showChevron: true,
        showDivider: true,
        onTap: () => {
          this.openInfoSheet('licenses');
        }
      })
      RowItem({
        symbol: 'list',
        title: '导出诊断日志',
        value: this.logPath.length > 0 ? '已导出' : '',
        subtitle: this.logPath.length > 0 ? this.logPath : '把最近的错误记录写成日志文件，便于反馈问题',
        showChevron: this.logPath.length === 0,
        showDivider: true,
        onTap: () => {
          this.exportLog();
        }
      })
      RowItem({
        symbol: 'sparkles',
        title: '应用',
        value: 'OpenLess 鸿蒙原生版',
        showDivider: true
      })
      RowItem({
        symbol: 'settings',
        title: '平台',
        value: this.capabilities.platform,
        showDivider: true
      })
      RowItem({
        symbol: 'localasr',
        title: '离线听写',
        value: this.capabilities.supportsLocalAsr === true ? '支持' : '端侧试用（主引擎需联网）',
        showDivider: true
      })
      RowItem({
        symbol: 'list',
        title: '后端契约',
        value: this.contract.length > 0 ? this.contract : '—',
        showDivider: false
      })
    }
  }

  /** iOS trailing version Section: one card, one centred footnote. */
  @Builder
  versionGroup() {
    Column({ space: Space.xs }) {
      Group({ radius: 12 }) {
        SettingsFootnoteRow({ label: this.versionLabel() })
      }
      // iOS Settings carries no toolbar item, so the reload action lives here.
      Row() {
        TextButton({ label: '重新读取', tone: 'quiet', onTap: () => this.reload() })
      }
      .width('100%')
      .justifyContent(FlexAlign.Center)
    }
    .width('100%')
  }

  // ── Sheets ──────────────────────────────────────────────────────────────────

  /** Hidden anchors for the three sheets; sheets stay inside the page tree. */
  @Builder
  sheets() {
    Row()
      .width(0)
      .height(0)
      .bindSheet($$this.micOpen, this.micSheet, {
        preferType: this.isWide() ? SheetType.CENTER : SheetType.BOTTOM,
        width: this.isWide() ? 480 : '100%',
        height: this.isWide() ? SheetSize.LARGE : SheetSize.MEDIUM,
        dragBar: true,
        radius: LengthMetrics.vp(Radius.sheet),
        showClose: true,
        onDisappear: () => {
          this.micOpen = false;
        }
      })

    Row()
      .width(0)
      .height(0)
      .bindSheet($$this.clearOpen, this.clearSheet, {
        preferType: this.isWide() ? SheetType.CENTER : SheetType.BOTTOM,
        width: this.isWide() ? 420 : '100%',
        height: this.isWide() ? SheetSize.FIT_CONTENT : SheetSize.MEDIUM,
        dragBar: true,
        radius: LengthMetrics.vp(Radius.sheet),
        showClose: true,
        onDisappear: () => {
          this.clearOpen = false;
        }
      })

    Row()
      .width(0)
      .height(0)
      .bindSheet($$this.syncDeleteOpen, this.syncDeleteSheet, {
        preferType: this.isWide() ? SheetType.CENTER : SheetType.BOTTOM,
        width: this.isWide() ? 420 : '100%',
        height: this.isWide() ? SheetSize.FIT_CONTENT : SheetSize.MEDIUM,
        dragBar: true,
        radius: LengthMetrics.vp(Radius.sheet),
        showClose: true,
        onDisappear: () => {
          this.syncDeleteOpen = false;
        }
      })

    Row()
      .width(0)
      .height(0)
      .bindSheet($$this.infoOpen, this.infoSheet, {
        preferType: this.isWide() ? SheetType.CENTER : SheetType.BOTTOM,
        width: this.isWide() ? 520 : '100%',
        height: this.isWide() ? SheetSize.LARGE : SheetSize.LARGE,
        dragBar: true,
        radius: LengthMetrics.vp(Radius.sheet),
        showClose: true,
        onDisappear: () => {
          this.infoOpen = false;
        }
      })
  }

  @Builder
  infoSheet() {
    Column({ space: Space.sm }) {
      SheetTitle({
        title: this.infoMode === 'licenses' ? '开源许可与组件署名' : '帮助中心',
        hint: this.infoMode === 'licenses' ? 'OpenLess 2.0 · AGPL-3.0-only' : 'OpenLess 鸿蒙原生版使用说明'
      })
      Scroll() {
        Column({ space: Space.md }) {
          if (this.infoMode === 'licenses') {
            Group() {
              RowItem({ title: 'OpenLess 2.0', subtitle: '依据 GNU AGPL-3.0-only 开源发布，零数据收集，直连自选模型服务商。', showDivider: true })
              RowItem({ title: 'Rime 输入法引擎', subtitle: 'BSD-3-Clause · 提供本地中文拼音输入与用户词库学习。', showDivider: true })
              RowItem({ title: 'Open Runde 字体', subtitle: 'SIL Open Font License 1.1', showDivider: true })
              RowItem({ title: 'Rare UI 参考设计', subtitle: 'MIT + Commons Clause + Attribution', showDivider: false })
            }
            Row({ space: Space.sm }) {
              TextButton({
                label: '打开官网文档',
                onTap: () => {
                  Api.openExternalUrl('https://openless.top/zh/docs/').catch(() => {});
                }
              })
              TextButton({
                label: 'GitHub 仓库',
                tone: 'quiet',
                onTap: () => {
                  Api.openExternalUrl('https://github.com/Open-Less/openless').catch(() => {});
                }
              })
            }
            .width('100%')
          } else {
            Group() {
              RowItem({ title: '1. 配置语音与模型', subtitle: '进入「大模型配置」添加转写引擎与润色模型（或切换到全模态单模型），点击测试调用通过后设为当前使用。', showDivider: true })
              RowItem({ title: '2. 开启 OpenLess 输入法', subtitle: '在「系统设置 → 系统和更新 → 语言和输入法」启用 OpenLess 输入法，在其他应用输入框内长按空格或点工具栏麦克风即可语音上屏。', showDivider: true })
              RowItem({ title: '3. 词典、纠错与风格市场', subtitle: '在「词典」维护专有名词，在「风格」管理自定义提示词或浏览风格市场，在「翻译目标」设置目标语言与默认润色模式。', showDivider: true })
              RowItem({ title: '4. 历史回放与重新润色', subtitle: '开启「保留录音以便重新转写」后，可在历史详情里回放录音、切换风格重新润色或重新转写。', showDivider: true })
              RowItem({ title: '5. 账户与云备份', subtitle: '在设置顶部登录 GitHub 后可手动上传/恢复词典、纠错规则与风格包；密钥仅保存在系统 Asset 密钥库，绝不上传。', showDivider: false })
            }
          }
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
      Text('这个操作不可撤销。云端加密快照不受影响；需要删除它请到「账户与云备份」组。')
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

  /** Destructive confirmation for the encrypted cloud snapshot. */
  @Builder
  syncDeleteSheet() {
    Column({ space: Space.sm }) {
      SheetTitle({
        title: '删除云端快照',
        hint: '当前云端修订号 ' + this.syncRevision.toString()
      })
      Text('删除后其他设备无法再从云端恢复偏好；本机偏好与历史不受影响。这个操作不可撤销。')
        .fontSize(Type.label)
        .lineHeight(Leading.label)
        .fontColor(Palette.text2())
        .width('100%')
      Row({ space: Space.sm }) {
        PrimaryButton({
          label: this.syncing ? '正在删除…' : '确认删除',
          destructive: true,
          usable: !this.syncing,
          onTap: () => {
            this.syncDelete();
          }
        })
        TextButton({
          label: '取消',
          tone: 'quiet',
          onTap: () => {
            this.syncDeleteOpen = false;
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
