import { Labels } from '../common/Labels';
import { C, R, S, T } from '../common/Tokens';
import { Card, GhostButton, PageTitle, Pill, SectionHeader } from '../components/Common';
import { symbolOf } from '../components/Icons';
import { Api, CredentialStatus } from '../data/Api';
import { Channel, SettingsSnapshot, UserPreferences } from '../data/Types';

/**
 * Provider credentials.
 *
 * Everything is written through `set_credential`, which stores the value in the
 * system keystore keyed by (account, provider channel) - never in a file, never in
 * the preference document. The account names are the same ones the desktop UI
 * uses, so a key entered here shows up on the desktop and vice versa.
 */
@Component
export struct CredentialsPage {
  @StorageLink('bp') bp: string = 'md';
  @StorageLink('backendRevision') revision: number = 0;

  @State private prefs: UserPreferences = {};
  @State private status: CredentialStatus = {};
  @State private asrChannels: Channel[] = [];
  @State private llmChannels: Channel[] = [];
  @State private saved: string = '';
  @State private error: string = '';

  aboutToAppear(): void {
    this.reload();
  }

  private async reload(): Promise<void> {
    try {
      const snapshot: SettingsSnapshot = await Api.settings();
      this.prefs = snapshot.preferences;
      this.status = await Api.credentialStatus();
      this.asrChannels = await Api.channels('asr');
      this.llmChannels = await Api.channels('llm');
      this.error = '';
    } catch (error) {
      this.error = (error as Error).message;
    }
  }

  private asrProvider(): string {
    return this.prefs.activeAsrProvider ?? '';
  }

  private llmProvider(): string {
    return this.prefs.activeLlmProvider ?? '';
  }

  /** Volcano Engine needs an app id plus an access token; everything else one key. */
  private isVolcengine(provider: string): boolean {
    return provider.indexOf('volcengine') >= 0;
  }

  private async save(account: string, provider: string, value: string): Promise<void> {
    if (value.length === 0) return;
    try {
      await Api.setCredential(account, value, provider);
      this.saved = account;
      this.status = await Api.credentialStatus();
      this.error = '';
    } catch (error) {
      this.error = (error as Error).message;
    }
  }

  private channelCount(kind: string): string {
    const list: Channel[] = kind === 'asr' ? this.asrChannels : this.llmChannels;
    return list.length.toString() + ' 个渠道';
  }

  build() {
    Column() {
      Row({ space: S.sm }) {
        Column({ space: S.xxs }) {
          Text('凭据')
            .fontSize(T.display)
            .fontWeight(FontWeight.Medium)
            .fontColor(C.textPrimary())
          Text('密钥进系统密钥库，不写进配置文件')
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

      if (this.error.length > 0) {
        Text(this.error)
          .fontSize(T.footnote)
          .fontColor(C.danger())
          .width('100%')
          .padding(S.sm)
          .backgroundColor(C.dangerSoft())
          .borderRadius(R.sm)
      }

      Scroll() {
        Column({ space: S.md }) {
          Card() {
            Column({ space: S.sm }) {
              SectionHeader({
                title: '状态',
                hint: '未配置凭据时听写会失败，历史里会显示错误码'
              })
              Row({ space: S.sm }) {
                Pill({
                  text: this.status.asrConfigured === true ? '转写已配置' : '转写未配置',
                  tone: this.status.asrConfigured === true ? 'success' : 'warning'
                })
                Pill({
                  text: this.status.llmConfigured === true ? '润色已配置' : '润色未配置',
                  tone: this.status.llmConfigured === true ? 'success' : 'warning'
                })
                Pill({ text: Labels.provider(this.asrProvider()), tone: 'accent' })
              }
            }
            .width('100%')
          }

          Card() {
            Column({ space: S.sm }) {
              SectionHeader({
                title: '转写引擎',
                hint: '当前：' + Labels.provider(this.asrProvider()) + ' · ' + this.channelCount('asr')
              })
              if (this.isVolcengine(this.asrProvider())) {
                SecretField({
                  label: 'App ID',
                  account: this.asrProvider() + '.appId',
                  provider: this.asrProvider(),
                  onSave: (account: string, provider: string, value: string) => {
                    this.save(account, provider, value);
                  }
                })
                SecretField({
                  label: 'Access Token',
                  account: this.asrProvider() + '.accessToken',
                  provider: this.asrProvider(),
                  onSave: (account: string, provider: string, value: string) => {
                    this.save(account, provider, value);
                  }
                })
              } else {
                SecretField({
                  label: 'API Key',
                  account: this.asrProvider() + '.apiKey',
                  provider: this.asrProvider(),
                  onSave: (account: string, provider: string, value: string) => {
                    this.save(account, provider, value);
                  }
                })
              }
              PlainField({
                label: '接入点',
                account: 'asr.endpoint',
                provider: this.asrProvider(),
                onSave: (account: string, provider: string, value: string) => {
                  this.save(account, provider, value);
                }
              })
              PlainField({
                label: '模型',
                account: 'asr.model',
                provider: this.asrProvider(),
                onSave: (account: string, provider: string, value: string) => {
                  this.save(account, provider, value);
                }
              })
            }
            .width('100%')
          }

          Card() {
            Column({ space: S.sm }) {
              SectionHeader({
                title: '润色模型',
                hint: '当前：' + Labels.provider(this.llmProvider()) + ' · ' + this.channelCount('llm')
              })
              SecretField({
                label: 'API Key',
                account: this.llmProvider() + '.apiKey',
                provider: this.llmProvider(),
                onSave: (account: string, provider: string, value: string) => {
                  this.save(account, provider, value);
                }
              })
              PlainField({
                label: '接入点',
                account: 'ark.endpoint',
                provider: this.llmProvider(),
                onSave: (account: string, provider: string, value: string) => {
                  this.save(account, provider, value);
                }
              })
              PlainField({
                label: '模型 ID',
                account: 'ark.model_id',
                provider: this.llmProvider(),
                onSave: (account: string, provider: string, value: string) => {
                  this.save(account, provider, value);
                }
              })
            }
            .width('100%')
          }

          if (this.saved.length > 0) {
            Row({ space: S.xs }) {
              SymbolGlyph(symbolOf('check'))
                .fontSize(14)
                .fontColor([C.success()])
              Text('已保存 ' + this.saved)
                .fontSize(T.caption)
                .fontColor(C.textSecondary())
            }
            .width('100%')
          }
        }
        .width('100%')
        .padding({ bottom: S.xxl })
      }
      .scrollBar(BarState.Off)
      .layoutWeight(1)
      .align(Alignment.Top)
    }
    .width('100%')
    .height('100%')
    .padding({ left: S.gutter(this.bp), right: S.gutter(this.bp) })
  }
}

/** Masked input; saves on submit or when the field loses focus. */
@Component
struct SecretField {
  @Prop label: string;
  @Prop account: string;
  @Prop provider: string;
  onSave: (account: string, provider: string, value: string) => void = (account: string, provider: string, value: string) => {
  };
  @State private draft: string = '';

  build() {
    Column({ space: S.xs }) {
      Text(this.label)
        .fontSize(T.caption)
        .fontColor(C.textTertiary())
      Row({ space: S.sm }) {
        TextInput({ placeholder: '粘贴后回车保存', text: this.draft })
          .type(InputType.Password)
          .fontSize(T.body)
          .height(44)
          .layoutWeight(1)
          .backgroundColor(C.sunken())
          .borderRadius(R.md)
          .onChange((value: string) => {
            this.draft = value;
          })
          .onSubmit(() => {
            this.commit();
          })
        GhostButton({
          label: '保存',
          usable: this.draft.length > 0,
          onTap: () => {
            this.commit();
          }
        })
      }
      .width('100%')
    }
    .width('100%')
  }

  private commit(): void {
    if (this.draft.length === 0) return;
    this.onSave(this.account, this.provider, this.draft.trim());
    this.draft = '';
  }
}

/** Endpoint / model id: not secret, but stored through the same keystore entry. */
@Component
struct PlainField {
  @Prop label: string;
  @Prop account: string;
  @Prop provider: string;
  onSave: (account: string, provider: string, value: string) => void = (account: string, provider: string, value: string) => {
  };
  @State private draft: string = '';

  build() {
    Column({ space: S.xs }) {
      Text(this.label)
        .fontSize(T.caption)
        .fontColor(C.textTertiary())
      Row({ space: S.sm }) {
        TextInput({ placeholder: '例如 https://... 或模型名', text: this.draft })
          .fontSize(T.body)
          .height(44)
          .layoutWeight(1)
          .backgroundColor(C.sunken())
          .borderRadius(R.md)
          .onChange((value: string) => {
            this.draft = value;
          })
          .onSubmit(() => {
            this.commit();
          })
        GhostButton({
          label: '保存',
          usable: this.draft.length > 0,
          onTap: () => {
            this.commit();
          }
        })
      }
      .width('100%')
    }
    .width('100%')
  }

  private commit(): void {
    if (this.draft.length === 0) return;
    this.onSave(this.account, this.provider, this.draft.trim());
    this.draft = '';
  }
}
