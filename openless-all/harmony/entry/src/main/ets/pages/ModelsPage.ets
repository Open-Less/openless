import { Labels } from '../common/Labels';
import { Layout, Space } from '../common/Theme';
import { AdaptiveColumns } from '../components/AdaptiveLayout';
import { Banner, ErrorNote, Group, PageScaffold, RowItem, SectionTitle, SettingSwitch, Surface, notify } from '../components/Kit';
import { SystemSpeechCard } from '../components/models/SystemSpeechCard';
import { Nav } from '../components/Nav';
import { Api } from '../data/Api';
import { PlatformCapabilities, SettingsSnapshot, UserPreferences } from '../data/Types';

/** System-owned offline ASR and Core-managed downloadable models are different
 * capabilities. A successful trial must not silently change the main engine.
 * https://developer.huawei.com/consumer/cn/doc/doccenter-references/api/hms-ai-speechrecognizer
 */
@Component
export struct LocalAsrPage {
  @StorageLink('bp') bp: string = Layout.SM;
  @StorageLink('phoneLayout') phoneLayout: boolean = true;
  @State private capabilities: PlatformCapabilities = { platform: 'harmony' };
  @State private prefs: UserPreferences = {};
  @State private error: string = '';
  @State private busy: boolean = false;

  aboutToAppear(): void {
    this.reload();
  }

  private async reload(): Promise<void> {
    try {
      this.capabilities = await Api.platformCapabilities();
      const settings: SettingsSnapshot = await Api.settings();
      this.prefs = settings.preferences;
      this.error = '';
    } catch (error) {
      this.error = (error as Error).message;
    }
  }

  private async enableLocal(value: boolean): Promise<void> {
    if (this.busy || this.capabilities.supportsLocalAsr !== true) return;
    this.busy = true;
    try {
      const settings: SettingsSnapshot = await Api.writeBool('/localAsrFallbackEnabled', value);
      this.prefs = settings.preferences;
      notify(value ? '已启用本地转写回退' : '已关闭本地转写回退');
    } catch (error) {
      this.error = (error as Error).message;
    } finally {
      this.busy = false;
    }
  }

  build() {
    PageScaffold({ title: '端侧模型', subtitle: '系统离线语音与模型能力',
      actionLabel: '刷新', actionSymbol: 'refresh', onAction: () => this.reload() }) {
      if (this.error.length > 0) {
        ErrorNote({ message: this.error, onRetry: () => this.reload() })
      }
      AdaptiveColumns({ bp: this.bp, singleColumn: this.phoneLayout, secondaryWidth: 280,
        primary: () => { this.modelContent() }, secondary: () => { this.engineContent() } })
    }
  }

  @Builder
  modelContent() {
    SystemSpeechCard({ onCopy: (text: string): void => {
      Api.copyText(text).then((): void => {
        notify('已复制识别结果');
      }).catch((): void => {
        notify('复制失败，请重试');
      });
    } })
    Surface() {
      SectionTitle({ title: '自选端侧模型', hint: '与系统内置语音服务分开管理' })
      if (this.capabilities.supportsLocalAsr === true) {
        SettingSwitch({ label: '启用核心本地转写', checked: this.prefs.localAsrEnabled === true,
          showDivider: false, onToggle: (value: boolean) => this.enableLocal(value) })
        RowItem({ title: '当前模型', value: this.prefs.localAsrModel ?? '未安装', showDivider: false })
      } else {
        Banner({ title: '自选模型尚未接入',
          text: '当前版本暂不提供模型下载、导入或切换。系统离线语音可在上方独立试用；它不代表已接入主听写流程。', tone: 'neutral' })
      }
    }
  }

  @Builder
  engineContent() {
    Group() {
      RowItem({ symbol: 'settings', title: '主听写引擎', value: Labels.provider(this.prefs.activeAsrProvider),
        subtitle: '试用端侧语音不会修改此配置', showChevron: true,
        onTap: () => Nav.go(Nav.CREDENTIALS) })
      RowItem({ title: '处理管线', value: Labels.pipeline(this.prefs.pipelineMode), showDivider: false,
        showChevron: true, onTap: () => Nav.go(Nav.CREDENTIALS) })
    }
    Banner({ title: '离线识别 ≠ 离线润色',
      text: '系统语音试用在设备上识别中文，结果只显示在本页，不写入历史、不自动上屏、不发送给云端润色服务。复制需要手动操作。', tone: 'neutral' })
    Banner({ title: '模型适配说明',
      text: '端侧模型需要匹配的权重、词表和推理实现。鸿蒙 MindSpore Lite 提供 CPU 推理及可选的硬件加速；拥有 NPU 不等于任意模型都能直接运行。', tone: 'neutral' })
  }
}
