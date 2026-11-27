import { Labels } from '../common/Labels';
import { Leading, Palette, Radius, Space, Type, floatShadow } from '../common/Theme';
import {
  EmptyHint,
  ErrorNote,
  Group,
  IconButton,
  InputField,
  PageScaffold,
  PrimaryButton,
  RowItem,
  SectionTitle,
  Segmented,
  Surface,
  Tag,
  TextButton,
  TonalButton,
  notify
} from '../components/Kit';
import { Api, CredentialStatus } from '../data/Api';
import { Channel, ChannelTest, ProviderDescriptor, SettingsSnapshot, UserPreferences } from '../data/Types';

/**
 * Providers that exist as descriptors but cannot run in the HarmonyOS host:
 * macOS OS recognizers, local runtimes (the host reports supportsLocalAsr=false)
 * and OAuth/SIWC logins that need a desktop-side browser flow.
 */
const UNSUPPORTED_PROVIDERS: string[] = [
  'apple-speech',
  'apple-whisper',
  'local-whisper',
  'foundry-local-whisper',
  'sherpa-onnx-local',
  'local-qwen3',
  'local-qwen3-mlx',
  'local-qwen3-c',
  'experimental-textstream',
  'experimental-sensevoice',
  'experimental-embedding-gemma'
];

/**
 * One keystore entry the edited channel needs. Accounts are the canonical ones
 * the Rust core reads at runtime (`asr.api_key`, `ark.api_key`, ...), keyed by
 * provider channel id at write time.
 */
interface FieldSpec {
  label: string;
  account: string;
  secret: boolean;
  placeholder: string;
}

/** Draft field: keeps its own text so the overlay does not re-render on keypress. */
@Component
struct CredentialInput {
  @Prop label: string;
  @Prop placeholder: string = '';
  @Prop secret: boolean = true;
  onInput: (value: string) => void = (value: string) => {
  };
  @State private draft: string = '';

  build() {
    InputField({
      label: this.label,
      placeholder: this.placeholder,
      value: this.draft,
      password: this.secret,
      onInput: (value: string) => {
        this.draft = value;
        this.onInput(value);
      }
    })
  }
}

/**
 * 大模型配置 - the single entry point for provider credentials.
 *
 * The page only lists channels that actually exist (never the full provider
 * catalogue). Adding and editing both happen in one overlay: a scrim dims and
 * blocks the page, the editor card floats above it, and the card owns provider
 * selection, naming, keystore fields, a real validation call and deletion.
 */
@Component
export struct CredentialsPage {
  @State private prefs: UserPreferences = {};
  @State private status: CredentialStatus = {};
  @State private asrChannels: Channel[] = [];
  @State private llmChannels: Channel[] = [];
  @State private asrDescriptors: ProviderDescriptor[] = [];
  @State private llmDescriptors: ProviderDescriptor[] = [];
  @State private error: string = '';
  @State private loading: boolean = false;

  // Overlay editor.
  @State private editorOpen: boolean = false;
  @State private editorStep: string = 'pick';
  @State private editorKind: string = 'llm';
  @State private editorChannelId: string = '';
  @State private editorProviderType: string = '';
  @State private editorName: string = '';
  @State private editorOriginalName: string = '';
  @State private editorActive: boolean = false;
  @State private editorSaving: boolean = false;
  @State private editorTesting: boolean = false;
  @State private editorTest: string = 'idle';
  @State private editorTestNote: string = '';
  @State private editorConfirmDelete: boolean = false;
  @State private editorError: string = '';
  /** True while the overlay is editing a channel it created in this same session. */
  @State private editorCreated: boolean = false;
  /** True once the overlay wrote anything, so "取消" keeps the channel. */
  @State private editorApplied: boolean = false;

  /** Draft values keyed by credential account; not rendered, so plain state. */
  private drafts: Map<string, string> = new Map<string, string>();

  aboutToAppear(): void {
    this.reload();
  }

  private async reload(): Promise<void> {
    this.loading = true;
    try {
      const snapshot: SettingsSnapshot = await Api.settings();
      this.prefs = snapshot.preferences;
      this.status = await Api.credentialStatus();
      this.asrChannels = await Api.channels('asr');
      this.llmChannels = await Api.channels('llm');
      this.asrDescriptors = await Api.providerDescriptors('asr');
      this.llmDescriptors = await Api.providerDescriptors('llm');
      this.error = '';
    } catch (error) {
      this.error = (error as Error).message;
    } finally {
      this.loading = false;
    }
  }

  private totalChannels(): number {
    return this.asrChannels.length + this.llmChannels.length;
  }

  private asrActive(): string {
    return this.prefs.activeAsrProvider ?? '';
  }

  private llmActive(): string {
    return this.prefs.activeLlmProvider ?? '';
  }

  private activeOf(kind: string): string {
    return kind === 'asr' ? this.asrActive() : this.llmActive();
  }

  private channelsOf(kind: string): Channel[] {
    return kind === 'asr' ? this.asrChannels : this.llmChannels;
  }

  private descriptorsOf(kind: string): ProviderDescriptor[] {
    return kind === 'asr' ? this.asrDescriptors : this.llmDescriptors;
  }

  private channelSubtitle(channel: Channel): string {
    const parts: string[] = [];
    parts.push(Labels.provider(channel.providerType));
    const test: ChannelTest | null | undefined = channel.lastTest;
    if (test !== undefined && test !== null) {
      parts.push(test.ok ? '上次调用通过' : '上次调用失败');
    } else {
      parts.push('还没测试过');
    }
    if (!channel.enabled) {
      parts.push('已停用');
    }
    return parts.join(' · ');
  }

  private channelValue(channel: Channel, activeId: string): string {
    if (channel.id === activeId) return '使用中';
    return channel.enabled ? '已启用' : '已停用';
  }

  /**
   * Provider options for the add overlay: drop what the HarmonyOS host cannot
   * run (OS recognizers, local runtimes, OAuth/SIWC flows) instead of listing
   * providers that would always fail. Falls back to the full list when the
   * filter would leave nothing to choose.
   */
  private providerOptions(kind: string): ProviderDescriptor[] {
    const all: ProviderDescriptor[] = this.descriptorsOf(kind);
    const kept: ProviderDescriptor[] = [];
    for (let index = 0; index < all.length; index++) {
      if (this.isSupported(all[index])) {
        kept.push(all[index]);
      }
    }
    return kept.length > 0 ? kept : all;
  }

  private isSupported(descriptor: ProviderDescriptor): boolean {
    if (UNSUPPORTED_PROVIDERS.indexOf(descriptor.providerType) >= 0) return false;
    const requirement: string = descriptor.authRequirement;
    if (requirement === 'none') return false;
    if (requirement === 'oauth') return false;
    if (requirement === 'siwc') return false;
    return true;
  }

  private descriptorHint(descriptor: ProviderDescriptor): string {
    const parts: string[] = [];
    parts.push(descriptor.providerType);
    const model: string | null | undefined = descriptor.defaultModel;
    if (model !== undefined && model !== null && model.length > 0) {
      parts.push('默认模型 ' + model);
    }
    return parts.join(' · ');
  }

  /** Volcano Engine (ASR) takes an app id plus a token; everything else one key. */
  private isVolcengine(providerType: string): boolean {
    return providerType.indexOf('volcengine') >= 0;
  }

  private fieldSpecs(kind: string, providerType: string): FieldSpec[] {
    const specs: FieldSpec[] = [];
    if (kind === 'asr') {
      if (this.isVolcengine(providerType)) {
        const appId: FieldSpec = {
          label: 'App ID',
          account: 'volcengine.app_key',
          secret: false,
          placeholder: '火山引擎语音控制台的 App ID'
        };
        specs.push(appId);
        const token: FieldSpec = {
          label: 'Access Token',
          account: 'volcengine.access_key',
          secret: true,
          placeholder: '粘贴后保存到系统密钥库'
        };
        specs.push(token);
      } else {
        const key: FieldSpec = {
          label: 'API Key',
          account: 'asr.api_key',
          secret: true,
          placeholder: '粘贴后保存到系统密钥库'
        };
        specs.push(key);
      }
      const endpoint: FieldSpec = {
        label: '接入点',
        account: 'asr.endpoint',
        secret: false,
        placeholder: '可选：不填就用服务商默认接入点'
      };
      specs.push(endpoint);
      const model: FieldSpec = {
        label: '模型',
        account: 'asr.model',
        secret: false,
        placeholder: '可选：不填就用服务商默认模型'
      };
      specs.push(model);
      return specs;
    }

    const key: FieldSpec = {
      label: 'API Key',
      account: 'ark.api_key',
      secret: true,
      placeholder: '粘贴后保存到系统密钥库'
    };
    specs.push(key);
    const endpoint: FieldSpec = {
      label: '接入点',
      account: 'ark.endpoint',
      secret: false,
      placeholder: '可选：不填就用服务商默认接入点'
    };
    specs.push(endpoint);
    const model: FieldSpec = {
      label: '模型 ID',
      account: 'ark.model_id',
      secret: false,
      placeholder: '可选，例如 doubao-seed-1.6'
    };
    specs.push(model);
    return specs;
  }

  // ── Overlay lifecycle ──────────────────────────────────────────────────────

  private openCreate(): void {
    this.editorOpen = true;
    this.editorStep = 'pick';
    this.editorKind = 'llm';
    this.editorChannelId = '';
    this.editorProviderType = '';
    this.editorName = '';
    this.editorOriginalName = '';
    this.editorActive = false;
    this.editorSaving = false;
    this.editorTesting = false;
    this.editorTest = 'idle';
    this.editorTestNote = '';
    this.editorConfirmDelete = false;
    this.editorError = '';
    this.editorCreated = false;
    this.editorApplied = false;
    this.drafts.clear();
  }

  private openEdit(kind: string, channel: Channel): void {
    this.editorOpen = true;
    this.editorError = '';
    this.enterEdit(kind, channel, false);
  }

  private enterEdit(kind: string, channel: Channel, created: boolean): void {
    this.editorStep = 'edit';
    this.editorKind = kind;
    this.editorChannelId = channel.id;
    this.editorProviderType = channel.providerType;
    this.editorName = channel.name;
    this.editorOriginalName = channel.name;
    this.editorActive = channel.id === this.activeOf(kind);
    this.editorSaving = false;
    this.editorTesting = false;
    this.editorTest = 'idle';
    this.editorTestNote = '';
    this.editorConfirmDelete = false;
    this.editorCreated = created;
    this.editorApplied = false;
    this.drafts.clear();
  }

  private closeEditor(): void {
    if (this.editorSaving || this.editorTesting) return;
    if (this.editorCreated && !this.editorApplied && this.editorChannelId.length > 0) {
      this.discardCreated();
      return;
    }
    this.finishClose();
  }

  /** 取消新建：先关掉覆盖层，再删掉这一层里刚创建、还没写入内容的空渠道。 */
  private async discardCreated(): Promise<void> {
    const kind: string = this.editorKind;
    const id: string = this.editorChannelId;
    this.finishClose();
    try {
      await Api.deleteChannel(kind, id);
    } catch (error) {
      console.warn('[OpenLess Credentials] discard created channel failed');
    }
    await this.reload();
  }

  private finishClose(): void {
    this.editorOpen = false;
    this.editorError = '';
    this.editorConfirmDelete = false;
    this.editorCreated = false;
    this.editorApplied = false;
    this.drafts.clear();
    this.reload();
  }

  private editorTitle(): string {
    if (this.editorStep === 'pick') return '添加大模型';
    const name: string = this.editorName.trim();
    return name.length > 0 ? name : Labels.provider(this.editorProviderType);
  }

  private editorSubtitle(): string {
    if (this.editorStep === 'pick') return '选一个服务商，再填名称、密钥与接入点';
    return (this.editorKind === 'asr' ? '转写引擎' : '润色模型') + ' · ' + Labels.provider(this.editorProviderType);
  }

  private editorConfigured(): boolean {
    if (this.editorKind === 'asr') return this.status.asrConfigured === true;
    return this.status.llmConfigured === true;
  }

  private testLabel(): string {
    if (this.editorTest === 'testing') return '检测中';
    if (this.editorTest === 'ok') return '通过';
    if (this.editorTest === 'fail') return '失败';
    return '未测试';
  }

  private testTone(): string {
    if (this.editorTest === 'ok') return 'success';
    if (this.editorTest === 'fail') return 'danger';
    return 'neutral';
  }

  // ── Overlay actions ────────────────────────────────────────────────────────

  /** Creates the channel, then switches the overlay to its edit step. */
  private async chooseProvider(providerType: string): Promise<void> {
    if (this.editorSaving) return;
    this.editorSaving = true;
    this.editorError = '';
    try {
      const channel: Channel = await Api.createChannel(this.editorKind, providerType, this.editorName.trim());
      if (this.editorKind === 'asr') {
        await Api.activateAsrProvider(channel.id);
      } else {
        await Api.activateLlmProvider(channel.id);
      }
      this.enterEdit(this.editorKind, channel, true);
      this.editorActive = true;
      notify('已新建渠道 ' + Labels.provider(providerType));
      await this.reload();
    } catch (error) {
      this.editorError = (error as Error).message;
    } finally {
      this.editorSaving = false;
    }
  }

  /** Writes the pending drafts (name + every non-empty field). */
  private async applyDrafts(): Promise<void> {
    const name: string = this.editorName.trim();
    if (name.length > 0 && name !== this.editorOriginalName) {
      await Api.renameChannel(this.editorKind, this.editorChannelId, name);
      this.editorOriginalName = name;
      this.editorApplied = true;
    }
    const specs: FieldSpec[] = this.fieldSpecs(this.editorKind, this.editorProviderType);
    for (let index = 0; index < specs.length; index++) {
      const account: string = specs[index].account;
      const raw: string | undefined = this.drafts.get(account);
      const value: string = raw === undefined ? '' : raw.trim();
      if (value.length > 0) {
        await Api.setCredential(account, value, this.editorChannelId);
        this.editorApplied = true;
      }
    }
  }

  private async saveEditor(): Promise<void> {
    if (this.editorSaving || this.editorTesting) return;
    if (this.editorChannelId.length === 0) return;
    this.editorSaving = true;
    this.editorError = '';
    try {
      await this.applyDrafts();
      this.status = await Api.credentialStatus();
      notify('已保存到系统密钥库');
      this.editorCreated = false;
      this.editorApplied = false;
      this.editorConfirmDelete = false;
      this.editorOpen = false;
      await this.reload();
    } catch (error) {
      this.editorError = (error as Error).message;
    } finally {
      this.editorSaving = false;
    }
  }

  /** Saves the drafts first, then runs the real provider validation call. */
  private async runTest(): Promise<void> {
    if (this.editorTesting || this.editorSaving) return;
    if (this.editorChannelId.length === 0) return;
    this.editorTesting = true;
    this.editorTest = 'testing';
    this.editorTestNote = '正在保存并按服务商真实接口校验…';
    this.editorError = '';
    const started: number = Date.now();
    try {
      await this.applyDrafts();
      const ok: boolean = await Api.validateChannel(this.editorKind, this.editorChannelId);
      const latency: number = Date.now() - started;
      await Api.recordChannelTest(this.editorKind, this.editorChannelId, ok, latency,
        ok ? '' : '校验未通过');
      this.editorTest = ok ? 'ok' : 'fail';
      this.editorTestNote = ok
        ? '调用通过 · ' + latency.toString() + ' ms，配置已可用'
        : '调用失败：请检查密钥、接入点与模型名';
      this.status = await Api.credentialStatus();
      await this.reload();
    } catch (error) {
      this.editorTest = 'fail';
      this.editorTestNote = '调用失败：' + (error as Error).message;
    } finally {
      this.editorTesting = false;
    }
  }

  private async activateEditorChannel(): Promise<void> {
    try {
      if (this.editorKind === 'asr') {
        await Api.activateAsrProvider(this.editorChannelId);
      } else {
        await Api.activateLlmProvider(this.editorChannelId);
      }
      this.editorActive = true;
      notify('已设为当前使用');
      await this.reload();
    } catch (error) {
      this.editorError = (error as Error).message;
    }
  }

  private async deleteEditorChannel(): Promise<void> {
    if (this.editorChannelId.length === 0) return;
    try {
      await Api.deleteChannel(this.editorKind, this.editorChannelId);
      notify('已删除渠道');
      this.editorOpen = false;
      this.editorConfirmDelete = false;
      this.editorCreated = false;
      this.editorApplied = false;
      await this.reload();
    } catch (error) {
      this.editorError = (error as Error).message;
    }
  }

  // ── Page ───────────────────────────────────────────────────────────────────

  build() {
    Stack({ alignContent: Alignment.Center }) {
      Column() {
        PageScaffold({
          title: '大模型配置',
          subtitle: '密钥进系统密钥库，不写进配置文件',
          actionLabel: '刷新',
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
          this.addBar()
          this.statusCard()
          if (this.totalChannels() === 0) {
            this.emptyCard()
          } else {
            if (this.asrChannels.length > 0) {
              this.channelGroup('asr')
            }
            if (this.llmChannels.length > 0) {
              this.channelGroup('llm')
            }
          }
        }
      }
      .width('100%')
      .height('100%')
      .enabled(!this.editorOpen)

      if (this.editorOpen) {
        Column()
          .width('100%')
          .height('100%')
          .backgroundColor(Palette.scrim())
          .onClick(() => {
            this.closeEditor();
          })
        this.editorCard()
      }
    }
    .width('100%')
    .height('100%')
  }

  @Builder
  statusCard() {
    Surface({ inset: Space.md }) {
      Column({ space: Space.sm }) {
        SectionTitle({
          title: '状态',
          hint: '已配置 ' + this.totalChannels().toString() + ' 个渠道，密钥只写不读'
        })
        Row({ space: Space.xs }) {
          Tag({
            text: this.status.asrConfigured === true ? '转写已配置' : '转写未配置',
            tone: this.status.asrConfigured === true ? 'success' : 'warning'
          })
          Tag({
            text: this.status.llmConfigured === true ? '润色已配置' : '润色未配置',
            tone: this.status.llmConfigured === true ? 'success' : 'warning'
          })
        }
        .width('100%')
        Row({ space: Space.xs }) {
          Tag({ text: '转写 · ' + Labels.provider(this.asrActive()), tone: 'accent' })
          Tag({ text: '润色 · ' + Labels.provider(this.llmActive()), tone: 'accent' })
        }
        .width('100%')
      }
      .width('100%')
    }
  }

  @Builder
  addBar() {
    Row({ space: Space.sm }) {
      PrimaryButton({
        label: this.loading ? '读取中…' : '添加大模型',
        usable: !this.loading && !this.editorOpen,
        onTap: () => {
          this.openCreate();
        }
      })
    }
    .width('100%')
  }

  @Builder
  emptyCard() {
    Surface({ inset: Space.md }) {
      Column({ space: Space.sm }) {
        EmptyHint({
          symbol: 'add',
          title: '还没有配置大模型',
          hint: '点上面的「添加大模型」，选一个服务商并填入密钥'
        })
      }
      .width('100%')
    }
  }

  @Builder
  channelGroup(kind: string) {
    Group() {
      Column() {
        Row() {
          SectionTitle({
            title: kind === 'asr' ? '转写引擎' : '润色模型',
            hint: '当前：' + Labels.provider(this.activeOf(kind)) + ' · ' + this.channelsOf(kind).length.toString() + ' 个渠道'
          })
        }
        .width('100%')
        .padding({ left: Space.md, right: Space.md, top: Space.sm, bottom: Space.xxs })

        ForEach(this.channelsOf(kind), (channel: Channel, index: number) => {
          RowItem({
            symbol: kind === 'asr' ? 'localasr' : 'style',
            accentSymbol: channel.id === this.activeOf(kind),
            title: channel.name.length > 0 ? channel.name : Labels.provider(channel.providerType),
            subtitle: this.channelSubtitle(channel),
            value: this.channelValue(channel, this.activeOf(kind)),
            showChevron: true,
            showDivider: index < this.channelsOf(kind).length - 1,
            onTap: () => {
              this.openEdit(kind, channel);
            }
          })
        }, (channel: Channel) => kind + '-' + channel.id)
      }
      .width('100%')
    }
  }

  // ── Overlay card ───────────────────────────────────────────────────────────

  @Builder
  editorCard() {
    Column() {
      Row({ space: Space.sm }) {
        Column({ space: Space.xxs }) {
          Text(this.editorTitle())
            .fontSize(Type.section)
            .lineHeight(Leading.section)
            .fontWeight(FontWeight.Medium)
            .fontColor(Palette.text1())
          Text(this.editorSubtitle())
            .fontSize(Type.caption)
            .lineHeight(Leading.caption)
            .fontColor(Palette.text3())
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)

        IconButton({
          symbol: 'close',
          onTap: () => {
            this.closeEditor();
          }
        })
      }
      .width('100%')

      Scroll() {
        Column({ space: Space.md }) {
          if (this.editorStep === 'pick') {
            this.pickStep()
          } else {
            this.editStep()
          }
        }
        .width('100%')
        .padding({ top: Space.md, bottom: Space.md })
        .alignItems(HorizontalAlign.Start)
      }
      .width('100%')
      .constraintSize({ maxHeight: 400 })
      .align(Alignment.Top)
      .scrollBar(BarState.Auto)

      if (this.editorError.length > 0) {
        Text(this.editorError)
          .fontSize(Type.label)
          .lineHeight(Leading.label)
          .fontColor(Palette.danger())
          .width('100%')
      }

      Row({ space: Space.sm }) {
        if (this.editorStep === 'edit') {
          PrimaryButton({
            label: this.editorSaving ? '保存中…' : '保存',
            usable: !this.editorSaving && !this.editorTesting,
            onTap: () => {
              this.saveEditor();
            }
          })
        }
        TextButton({
          label: '取消',
          tone: 'quiet',
          usable: !this.editorSaving,
          onTap: () => {
            this.closeEditor();
          }
        })
      }
      .width('100%')
      .justifyContent(FlexAlign.End)
      .padding({ top: Space.sm })
    }
    .width('92%')
    .constraintSize({ maxWidth: 560, maxHeight: 640 })
    .padding(Space.md)
    .backgroundColor(Palette.bgElevated())
    .borderRadius(Radius.sheet)
    .border({ width: 0.5, color: Palette.line() })
    .shadow(floatShadow())
    .alignItems(HorizontalAlign.Start)
  }

  @Builder
  pickStep() {
    Column({ space: Space.md }) {
      SectionTitle({ title: '类型', hint: '同一个覆盖层里配置转写引擎或润色模型' })
      Segmented({
        items: ['润色模型', '转写引擎'],
        selected: this.editorKind === 'llm' ? 0 : 1,
        onSelect: (index: number) => {
          this.editorKind = index === 0 ? 'llm' : 'asr';
        }
      })
      InputField({
        label: '名称',
        placeholder: '可选，例如「我的 GPT 渠道」',
        value: this.editorName,
        onInput: (value: string) => {
          this.editorName = value;
        }
      })
      SectionTitle({
        title: '服务商',
        hint: this.providerOptions(this.editorKind).length.toString() + ' 个可用（已隐藏本机不支持的服务商）'
      })
      if (this.providerOptions(this.editorKind).length === 0) {
        EmptyHint({
          symbol: 'list',
          title: '读不到服务商列表',
          hint: '后端可能还没就绪，先刷新这一页'
        })
      } else {
        Group() {
          Column() {
            ForEach(this.providerOptions(this.editorKind), (descriptor: ProviderDescriptor, index: number) => {
              RowItem({
                title: Labels.provider(descriptor.providerType),
                subtitle: this.descriptorHint(descriptor),
                showChevron: true,
                showDivider: index < this.providerOptions(this.editorKind).length - 1,
                onTap: () => {
                  this.chooseProvider(descriptor.providerType);
                }
              })
            }, (descriptor: ProviderDescriptor) => this.editorKind + '-pick-' + descriptor.providerType)
          }
          .width('100%')
        }
      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
  }

  @Builder
  editStep() {
    Column({ space: Space.md }) {
      Row({ space: Space.xs }) {
        Tag({ text: this.editorKind === 'asr' ? '转写引擎' : '润色模型', tone: 'accent' })
        Tag({
          text: this.editorActive ? '当前使用' : '未启用',
          tone: this.editorActive ? 'success' : 'neutral'
        })
        if (this.editorActive) {
          Tag({
            text: this.editorConfigured() ? '密钥已配置' : '密钥未配置',
            tone: this.editorConfigured() ? 'success' : 'warning'
          })
        }
      }
      .width('100%')

      Text('已保存的密钥不会回显；留空表示保持原值，填入即覆盖。')
        .fontSize(Type.caption)
        .lineHeight(Leading.caption)
        .fontColor(Palette.text3())
        .width('100%')

      InputField({
        label: '名称',
        placeholder: '给这个渠道起个名字',
        value: this.editorName,
        onInput: (value: string) => {
          this.editorName = value;
        }
      })

      if (!this.editorActive) {
        Row({ space: Space.sm }) {
          TonalButton({
            label: '设为当前使用',
            symbol: 'check',
            onTap: () => {
              this.activateEditorChannel();
            }
          })
        }
        .width('100%')
      }

      SectionTitle({ title: '凭据与接入点', hint: '写进系统密钥库，按渠道隔离' })

      ForEach(this.fieldSpecs(this.editorKind, this.editorProviderType), (spec: FieldSpec) => {
        CredentialInput({
          label: spec.label,
          placeholder: spec.placeholder,
          secret: spec.secret,
          onInput: (value: string) => {
            this.drafts.set(spec.account, value);
          }
        })
      }, (spec: FieldSpec) => this.editorChannelId + '-' + spec.account)

      Row({ space: Space.sm }) {
        TonalButton({
          label: this.editorTesting ? '检测中…' : '测试调用',
          symbol: 'play',
          usable: !this.editorTesting && !this.editorSaving,
          onTap: () => {
            this.runTest();
          }
        })
        Tag({ text: this.testLabel(), tone: this.testTone() })
      }
      .width('100%')

      if (this.editorTestNote.length > 0) {
        Text(this.editorTestNote)
          .fontSize(Type.label)
          .lineHeight(Leading.label)
          .fontColor(this.editorTest === 'fail' ? Palette.danger() : Palette.text2())
          .width('100%')
      }

      if (this.editorConfirmDelete) {
        Column({ space: Space.sm }) {
          Tag({ text: '确认删除这个渠道？', tone: 'danger' })
          Row({ space: Space.sm }) {
            PrimaryButton({
              label: '删除',
              destructive: true,
              onTap: () => {
                this.deleteEditorChannel();
              }
            })
            TextButton({
              label: '取消',
              tone: 'quiet',
              onTap: () => {
                this.editorConfirmDelete = false;
              }
            })
          }
          .width('100%')
        }
        .width('100%')
        .alignItems(HorizontalAlign.Start)
      } else {
        Row({ space: Space.sm }) {
          TextButton({
            label: '删除这个渠道',
            symbol: 'trash',
            tone: 'danger',
            onTap: () => {
              this.editorConfirmDelete = true;
            }
          })
        }
        .width('100%')
      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
  }
}
