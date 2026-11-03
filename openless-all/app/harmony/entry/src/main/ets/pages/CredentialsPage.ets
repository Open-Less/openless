import { Labels } from '../common/Labels';
import { Layout, Leading, Palette, Space, Type } from '../common/Theme';
import {
  EmptyHint,
  ErrorNote,
  Group,
  InputField,
  PageScaffold,
  PrimaryButton,
  RowItem,
  SectionTitle,
  SheetTitle,
  Surface,
  Tag,
  TextButton,
  TonalButton,
  notify
} from '../components/Kit';
import { Api, CredentialStatus, MarketplaceAuth } from '../data/Api';
import { Channel, ChannelTest, ProviderDescriptor, SettingsSnapshot, UserPreferences } from '../data/Types';

/** One keystore entry the current channel needs. */
interface FieldSpec {
  label: string;
  account: string;
  provider: string;
  secret: boolean;
  placeholder: string;
}

/**
 * Credentials v2.
 *
 * Everything is written through `set_credential`, which stores the value in the
 * system keystore keyed by (account, provider channel) - never in a file, never in
 * the preference document. Channels are listed per kind (转写 / 润色) and all of
 * their actions (启用 / 测试 / 拉取模型 / 删除) live in one bottom sheet instead of
 * a row of inline buttons, so the list stays readable on a phone.
 */
@Component
struct CredentialField {
  @Prop label: string;
  @Prop placeholder: string = '';
  @Prop account: string = '';
  @Prop provider: string = '';
  @Prop secret: boolean = true;
  onSave: (account: string, provider: string, value: string) => void =
    (account: string, provider: string, value: string) => {
    };
  @State private draft: string = '';
  @State private saved: boolean = false;

  private commit(): void {
    const value: string = this.draft.trim();
    if (value.length === 0) return;
    this.onSave(this.account, this.provider, value);
    this.draft = '';
    this.saved = true;
  }

  build() {
    Column({ space: Space.xs }) {
      InputField({
        label: this.label,
        placeholder: this.placeholder,
        value: this.draft,
        password: this.secret,
        onInput: (value: string) => {
          this.draft = value;
          this.saved = false;
        }
      })
      Row({ space: Space.sm }) {
        TonalButton({
          label: '保存',
          usable: this.draft.trim().length > 0,
          onTap: () => {
            this.commit();
          }
        })
        if (this.saved) {
          Tag({ text: '已保存 · 输入框已脱敏', tone: 'success' })
        }
      }
      .width('100%')
      .alignItems(VerticalAlign.Center)
    }
    .width('100%')
  }
}

@Component
export struct CredentialsPage {
  @StorageLink('bp') bp: string = 'md';

  @State private prefs: UserPreferences = {};
  @State private status: CredentialStatus = {};
  @State private asrChannels: Channel[] = [];
  @State private llmChannels: Channel[] = [];
  @State private asrDescriptors: ProviderDescriptor[] = [];
  @State private llmDescriptors: ProviderDescriptor[] = [];
  @State private error: string = '';
  @State private testing: boolean = false;

  // Sheets: create a channel / act on one / confirm deletion / pick a model.
  @State private createOpen: boolean = false;
  @State private createKind: string = 'asr';
  @State private actionOpen: boolean = false;
  @State private actionKind: string = 'asr';
  @State private actionId: string = '';
  @State private actionName: string = '';
  @State private confirmOpen: boolean = false;
  @State private modelsOpen: boolean = false;
  @State private models: string[] = [];
  @State private modelKind: string = 'asr';
  @State private modelChannel: string = '';

  // GitHub account (风格市场 / 云同步).
  @State private loggedIn: boolean = false;
  @State private loginName: string = '';
  @State private loginCode: string = '';

  aboutToAppear(): void {
    this.reload();
  }

  private isWide(): boolean {
    return this.bp === Layout.LG || this.bp === Layout.XL;
  }

  private async reload(): Promise<void> {
    try {
      const snapshot: SettingsSnapshot = await Api.settings();
      this.prefs = snapshot.preferences;
      this.status = await Api.credentialStatus();
      this.asrChannels = await Api.channels('asr');
      this.llmChannels = await Api.channels('llm');
      this.asrDescriptors = await Api.providerDescriptors('asr');
      this.llmDescriptors = await Api.providerDescriptors('llm');
      this.error = '';
      await this.loadLogin();
    } catch (error) {
      this.error = (error as Error).message;
    }
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

  private createDescriptors(): ProviderDescriptor[] {
    return this.createKind === 'asr' ? this.asrDescriptors : this.llmDescriptors;
  }

  private channelHint(kind: string): string {
    const channels: Channel[] = this.channelsOf(kind);
    return '当前：' + Labels.provider(this.activeOf(kind)) + ' · ' + channels.length.toString() + ' 个渠道';
  }

  /** Volcano Engine needs an app id plus an access token; everything else one key. */
  private isVolcengine(provider: string): boolean {
    return provider.indexOf('volcengine') >= 0;
  }

  private asrFields(): FieldSpec[] {
    const provider: string = this.asrActive();
    const specs: FieldSpec[] = [];
    if (this.isVolcengine(provider)) {
      const appId: FieldSpec = {
        label: 'App ID',
        account: provider + '.appId',
        provider: provider,
        secret: false,
        placeholder: '火山引擎控制台的 App ID'
      };
      specs.push(appId);
      const token: FieldSpec = {
        label: 'Access Token',
        account: provider + '.accessToken',
        provider: provider,
        secret: true,
        placeholder: '粘贴后保存到系统密钥库'
      };
      specs.push(token);
    } else {
      const key: FieldSpec = {
        label: 'API Key',
        account: provider + '.apiKey',
        provider: provider,
        secret: true,
        placeholder: '粘贴后保存到系统密钥库'
      };
      specs.push(key);
    }
    const endpoint: FieldSpec = {
      label: '接入点',
      account: 'asr.endpoint',
      provider: provider,
      secret: false,
      placeholder: '可选：自定义接入点'
    };
    specs.push(endpoint);
    const model: FieldSpec = {
      label: '模型',
      account: 'asr.model',
      provider: provider,
      secret: false,
      placeholder: '可选，例如 whisper-1'
    };
    specs.push(model);
    return specs;
  }

  private llmFields(): FieldSpec[] {
    const provider: string = this.llmActive();
    const specs: FieldSpec[] = [];
    const key: FieldSpec = {
      label: 'API Key',
      account: provider + '.apiKey',
      provider: provider,
      secret: true,
      placeholder: '粘贴后保存到系统密钥库'
    };
    specs.push(key);
    const endpoint: FieldSpec = {
      label: '接入点',
      account: 'ark.endpoint',
      provider: provider,
      secret: false,
      placeholder: '可选：自定义接入点'
    };
    specs.push(endpoint);
    const model: FieldSpec = {
      label: '模型 ID',
      account: 'ark.model_id',
      provider: provider,
      secret: false,
      placeholder: '可选，例如 doubao-seed-1.6'
    };
    specs.push(model);
    return specs;
  }

  private async save(account: string, provider: string, value: string): Promise<void> {
    if (value.length === 0) return;
    try {
      await Api.setCredential(account, value, provider);
      this.status = await Api.credentialStatus();
      this.error = '';
      notify('已保存到系统密钥库');
    } catch (error) {
      this.error = (error as Error).message;
    }
  }

  private async addChannel(kind: string, providerType: string): Promise<void> {
    try {
      const channel: Channel = await Api.createChannel(kind, providerType, '');
      if (kind === 'asr') {
        await Api.activateAsrProvider(channel.id);
      } else {
        await Api.activateLlmProvider(channel.id);
      }
      this.createOpen = false;
      this.error = '';
      notify('已新建渠道 ' + Labels.provider(providerType));
      await this.reload();
    } catch (error) {
      this.error = (error as Error).message;
    }
  }

  private async activate(kind: string, id: string): Promise<void> {
    try {
      if (kind === 'asr') {
        await Api.activateAsrProvider(id);
      } else {
        await Api.activateLlmProvider(id);
      }
      this.actionOpen = false;
      this.error = '';
      notify('已切换到这个渠道');
      await this.reload();
    } catch (error) {
      this.error = (error as Error).message;
    }
  }

  private async removeChannel(kind: string, id: string): Promise<void> {
    try {
      await Api.deleteChannel(kind, id);
      this.confirmOpen = false;
      this.actionOpen = false;
      this.error = '';
      notify('已删除渠道');
      await this.reload();
    } catch (error) {
      this.error = (error as Error).message;
    }
  }

  private async testChannel(kind: string, id: string): Promise<void> {
    if (this.testing || id.length === 0) return;
    this.testing = true;
    const started: number = Date.now();
    try {
      const ok: boolean = await Api.validateChannel(kind, id);
      const latency: number = Date.now() - started;
      await Api.recordChannelTest(kind, id, ok, latency, ok ? '' : '校验未通过');
      notify(ok ? '连通正常 · ' + latency.toString() + ' ms' : '校验失败，请检查密钥与接入点');
      await this.reload();
    } catch (error) {
      this.error = (error as Error).message;
      notify('测试失败');
    } finally {
      this.testing = false;
    }
  }

  private async loadModels(kind: string, channelId: string): Promise<void> {
    if (channelId.length === 0) {
      notify('先选择一个渠道');
      return;
    }
    this.modelKind = kind;
    this.modelChannel = channelId;
    this.models = [];
    try {
      const list: string[] = await Api.listModels(kind, channelId);
      this.models = list;
      if (list.length === 0) {
        notify('这个服务商不返回模型列表，直接手填模型名即可');
        return;
      }
      this.actionOpen = false;
      this.modelsOpen = true;
    } catch (error) {
      this.error = (error as Error).message;
      notify('拉取模型失败');
    }
  }

  private async applyModel(model: string): Promise<void> {
    const account: string = this.modelKind === 'asr' ? 'asr.model' : 'ark.model_id';
    this.modelsOpen = false;
    await this.save(account, this.modelChannel, model);
  }

  private openChannel(kind: string, channel: Channel): void {
    this.actionKind = kind;
    this.actionId = channel.id;
    this.actionName = channel.name.length > 0 ? channel.name : Labels.provider(channel.providerType);
    this.actionOpen = true;
  }

  private isActionActive(): boolean {
    return this.actionId.length > 0 && this.actionId === this.activeOf(this.actionKind);
  }

  private channelSubtitle(channel: Channel): string {
    const parts: string[] = [];
    parts.push(Labels.provider(channel.providerType));
    const test: ChannelTest | null | undefined = channel.lastTest;
    if (test !== undefined && test !== null) {
      parts.push(test.ok ? '上次连通正常' : '上次校验失败');
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

  private descriptorHint(descriptor: ProviderDescriptor): string {
    const parts: string[] = [];
    parts.push(descriptor.providerType);
    const model: string | null | undefined = descriptor.defaultModel;
    if (model !== undefined && model !== null && model.length > 0) {
      parts.push('默认模型 ' + model);
    }
    if (descriptor.authRequirement.length > 0) {
      parts.push('认证 · ' + descriptor.authRequirement);
    }
    return parts.join(' · ');
  }

  private accountLabel(): string {
    if (!this.loggedIn) return '未登录';
    return this.loginName.length > 0 ? '已登录 ' + this.loginName : '已登录';
  }

  private async loadLogin(): Promise<void> {
    try {
      const auth: MarketplaceAuth = await Api.marketplaceAuthStatus();
      this.loggedIn = auth.loggedIn === true;
      this.loginName = auth.login ?? '';
    } catch (error) {
      this.loggedIn = false;
      this.loginName = '';
    }
  }

  /** GitHub device flow: the code is copied so the user can paste it in the browser. */
  private async startLogin(): Promise<void> {
    try {
      const flow = await Api.githubDeviceFlowStart();
      this.loginCode = flow.userCode;
      Api.copyText(flow.userCode);
      notify('验证码已复制，去 ' + flow.verificationUri + ' 输入');
      let attempts = 0;
      while (attempts < 60) {
        attempts++;
        await new Promise<void>((resolve) => setTimeout(() => resolve(), Math.max(2, flow.interval) * 1000));
        const poll = await Api.githubDeviceFlowPoll(flow.flowId);
        if (poll.state === 'authorized') {
          this.loginName = poll.login ?? '';
          this.loggedIn = true;
          this.loginCode = '';
          notify('已登录 ' + this.loginName);
          return;
        }
        if (poll.state === 'expired' || poll.state === 'error') {
          this.loggedIn = false;
          this.loginCode = '';
          notify('登录失败或已过期');
          return;
        }
      }
      this.loginCode = '';
      notify('登录超时');
    } catch (error) {
      this.error = (error as Error).message;
      notify('登录不可用');
    }
  }

  private async logout(): Promise<void> {
    try {
      await Api.marketplaceLogout();
      this.loggedIn = false;
      this.loginName = '';
      this.loginCode = '';
      notify('已退出登录');
    } catch (error) {
      this.error = (error as Error).message;
    }
  }

  build() {
    PageScaffold({
      title: '凭据',
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
      this.statusCard()
      if (this.isWide()) {
        Row({ space: Space.md }) {
          Column({ space: Space.md }) {
            this.channelCard('asr')
            this.secretCard('asr')
            this.helpCard()
          }
          .layoutWeight(5)
          .alignItems(HorizontalAlign.Start)

          Column({ space: Space.md }) {
            this.channelCard('llm')
            this.secretCard('llm')
            this.accountCard()
          }
          .layoutWeight(4)
          .alignItems(HorizontalAlign.Start)
        }
        .width('100%')
        .alignItems(VerticalAlign.Top)
      } else {
        Column({ space: Space.md }) {
          this.channelCard('asr')
          this.secretCard('asr')
          this.channelCard('llm')
          this.secretCard('llm')
          this.accountCard()
          this.helpCard()
        }
        .width('100%')
      }
      this.sheetHost()
    }
  }

  @Builder
  statusCard() {
    Surface({ inset: Space.md }) {
      Column({ space: Space.sm }) {
        SectionTitle({ title: '状态', hint: '未配置凭据时听写会失败，历史里会显示错误码' })
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
  channelCard(kind: string) {
    Group() {
      Column() {
        Row() {
          SectionTitle({
            title: kind === 'asr' ? '转写渠道' : '润色渠道',
            hint: this.channelHint(kind)
          })
        }
        .width('100%')
        .padding({ left: Space.md, right: Space.md, top: Space.sm, bottom: Space.xxs })

        if (this.channelsOf(kind).length === 0) {
          EmptyHint({
            symbol: 'add',
            title: '还没有渠道',
            hint: '点下面的「新建渠道」，选一个服务商'
          })
        } else {
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
                this.openChannel(kind, channel);
              }
            })
          }, (channel: Channel) => kind + '-' + channel.id)
        }

        Row({ space: Space.sm }) {
          TonalButton({
            label: '新建渠道',
            symbol: 'add',
            onTap: () => {
              this.createKind = kind;
              this.createOpen = true;
            }
          })
        }
        .width('100%')
        .padding({ left: Space.md, right: Space.md, top: Space.sm, bottom: Space.md })
      }
      .width('100%')
    }
  }

  @Builder
  secretCard(kind: string) {
    Surface({ inset: Space.md }) {
      Column({ space: Space.md }) {
        SectionTitle({
          title: kind === 'asr' ? '转写凭据' : '润色凭据',
          hint: '按渠道隔离存进系统密钥库，保存后不再回显'
        })
        ForEach(kind === 'asr' ? this.asrFields() : this.llmFields(), (spec: FieldSpec) => {
          CredentialField({
            label: spec.label,
            placeholder: spec.placeholder,
            account: spec.account,
            provider: spec.provider,
            secret: spec.secret,
            onSave: (account: string, provider: string, value: string) => {
              this.save(account, provider, value);
            }
          })
        }, (spec: FieldSpec) => spec.account)
        Row({ space: Space.sm }) {
          TonalButton({
            label: '拉取可用模型',
            symbol: 'list',
            usable: this.activeOf(kind).length > 0,
            onTap: () => {
              this.loadModels(kind, this.activeOf(kind));
            }
          })
          TextButton({
            label: this.testing ? '测试中…' : '测试当前渠道',
            tone: 'quiet',
            usable: !this.testing && this.activeOf(kind).length > 0,
            onTap: () => {
              this.testChannel(kind, this.activeOf(kind));
            }
          })
        }
        .width('100%')
      }
      .width('100%')
    }
  }

  @Builder
  accountCard() {
    Surface({ inset: Space.md }) {
      Column({ space: Space.sm }) {
        SectionTitle({ title: '账号', hint: 'GitHub 账号用于风格市场与云同步' })
        Row({ space: Space.xs }) {
          Tag({ text: this.accountLabel(), tone: this.loggedIn ? 'success' : 'neutral' })
        }
        .width('100%')
        if (this.loginCode.length > 0) {
          Row({ space: Space.xs }) {
            Tag({ text: '验证码 ' + this.loginCode, tone: 'accent' })
            TextButton({
              label: '复制验证码',
              symbol: 'copy',
              onTap: () => {
                Api.copyText(this.loginCode);
                notify('已复制验证码');
              }
            })
          }
          .width('100%')
        }
        Row({ space: Space.sm }) {
          if (this.loggedIn) {
            TextButton({
              label: '退出登录',
              symbol: 'person',
              tone: 'danger',
              onTap: () => {
                this.logout();
              }
            })
          } else {
            TonalButton({
              label: 'GitHub 登录',
              symbol: 'person',
              onTap: () => {
                this.startLogin();
              }
            })
          }
        }
        .width('100%')
      }
      .width('100%')
    }
  }

  @Builder
  helpCard() {
    Surface({ tone: 'alt', inset: Space.md }) {
      Column({ space: Space.xs }) {
        SectionTitle({ title: '密钥怎么存' })
        Text('每个键值都通过 set_credential 写进系统密钥库，按（账号, 渠道）隔离；配置文件里只有偏好，没有密钥。保存后输入框清空，页面只显示「已配置」状态，不会回显明文。')
          .fontSize(Type.label)
          .lineHeight(Leading.body)
          .fontColor(Palette.text2())
          .width('100%')
      }
      .width('100%')
    }
  }

  /** Zero-size hosts: sheets are page-level, their content reads the state above. */
  @Builder
  sheetHost() {
    Column() {
      Row()
        .width(0)
        .height(0)
        .bindSheet($$this.createOpen, this.createSheet, {
          height: SheetSize.MEDIUM,
          dragBar: true,
          showClose: true,
          onDisappear: () => {
            this.createOpen = false;
          }
        })
      Row()
        .width(0)
        .height(0)
        .bindSheet($$this.actionOpen, this.actionSheet, {
          height: SheetSize.MEDIUM,
          dragBar: true,
          showClose: true,
          onDisappear: () => {
            this.actionOpen = false;
          }
        })
      Row()
        .width(0)
        .height(0)
        .bindSheet($$this.confirmOpen, this.confirmSheet, {
          height: SheetSize.MEDIUM,
          dragBar: true,
          showClose: true,
          onDisappear: () => {
            this.confirmOpen = false;
          }
        })
      Row()
        .width(0)
        .height(0)
        .bindSheet($$this.modelsOpen, this.modelsSheet, {
          height: SheetSize.MEDIUM,
          dragBar: true,
          showClose: true,
          onDisappear: () => {
            this.modelsOpen = false;
          }
        })
    }
    .width('100%')
  }

  @Builder
  createSheet() {
    Column({ space: Space.md }) {
      SheetTitle({
        title: '新建渠道',
        hint: this.createKind === 'asr' ? '选一个转写服务商' : '选一个润色服务商'
      })
      Text('新建后会立刻设为当前使用；密钥与接入点在下面的卡片里填。')
        .fontSize(Type.label)
        .lineHeight(Leading.label)
        .fontColor(Palette.text3())
        .width('100%')
      Scroll() {
        Group() {
          Column() {
            ForEach(this.createDescriptors(), (descriptor: ProviderDescriptor, index: number) => {
              RowItem({
                title: Labels.provider(descriptor.providerType),
                subtitle: this.descriptorHint(descriptor),
                showChevron: true,
                showDivider: index < this.createDescriptors().length - 1,
                onTap: () => {
                  this.addChannel(this.createKind, descriptor.providerType);
                }
              })
            }, (descriptor: ProviderDescriptor) => this.createKind + '-new-' + descriptor.providerType)
          }
          .width('100%')
        }
      }
      .width('100%')
      .constraintSize({ maxHeight: 420 })
      .scrollBar(BarState.Auto)
    }
    .width('100%')
    .padding({ left: Space.md, right: Space.md, top: Space.sm, bottom: Space.xl })
    .alignItems(HorizontalAlign.Start)
  }

  @Builder
  actionSheet() {
    Column({ space: Space.md }) {
      SheetTitle({
        title: this.actionName,
        hint: (this.actionKind === 'asr' ? '转写渠道' : '润色渠道') + ' · ' + this.actionId
      })
      Column({ space: Space.sm }) {
        if (!this.isActionActive()) {
          PrimaryButton({
            label: '启用这个渠道',
            onTap: () => {
              this.activate(this.actionKind, this.actionId);
            }
          })
        } else {
          Tag({ text: '当前正在使用', tone: 'success' })
        }
        Row({ space: Space.sm }) {
          TonalButton({
            label: this.testing ? '测试中…' : '测试连通',
            symbol: 'check',
            usable: !this.testing,
            onTap: () => {
              this.testChannel(this.actionKind, this.actionId);
            }
          })
          TonalButton({
            label: '拉取模型',
            symbol: 'list',
            onTap: () => {
              this.loadModels(this.actionKind, this.actionId);
            }
          })
        }
        .width('100%')
        TextButton({
          label: '删除渠道',
          symbol: 'trash',
          tone: 'danger',
          onTap: () => {
            this.confirmOpen = true;
          }
        })
      }
      .width('100%')
    }
    .width('100%')
    .padding({ left: Space.md, right: Space.md, top: Space.sm, bottom: Space.xl })
    .alignItems(HorizontalAlign.Start)
  }

  @Builder
  confirmSheet() {
    Column({ space: Space.md }) {
      SheetTitle({
        title: '删除这个渠道？',
        hint: '只移除本机的渠道配置；已保存的密钥仍留在系统密钥库，也不会被上传。'
      })
      Row({ space: Space.sm }) {
        PrimaryButton({
          label: '删除渠道',
          destructive: true,
          onTap: () => {
            this.removeChannel(this.actionKind, this.actionId);
          }
        })
        TextButton({
          label: '取消',
          tone: 'quiet',
          onTap: () => {
            this.confirmOpen = false;
          }
        })
      }
      .width('100%')
    }
    .width('100%')
    .padding({ left: Space.md, right: Space.md, top: Space.sm, bottom: Space.xl })
    .alignItems(HorizontalAlign.Start)
  }

  @Builder
  modelsSheet() {
    Column({ space: Space.md }) {
      SheetTitle({
        title: '可用模型',
        hint: this.models.length.toString() + ' 个 · 点一个写进凭据'
      })
      if (this.models.length === 0) {
        EmptyHint({
          symbol: 'list',
          title: '没有拿到模型列表',
          hint: '这个服务商可能不提供模型枚举，直接在凭据卡片里手填模型名'
        })
      } else {
        Scroll() {
          Group() {
            Column() {
              ForEach(this.models, (model: string, index: number) => {
                RowItem({
                  title: model,
                  value: '选用',
                  showChevron: true,
                  showDivider: index < this.models.length - 1,
                  onTap: () => {
                    this.applyModel(model);
                  }
                })
              }, (model: string) => 'model-' + model)
            }
            .width('100%')
          }
        }
        .width('100%')
        .constraintSize({ maxHeight: 420 })
        .scrollBar(BarState.Auto)
      }
    }
    .width('100%')
    .padding({ left: Space.md, right: Space.md, top: Space.sm, bottom: Space.xl })
    .alignItems(HorizontalAlign.Start)
  }
}
