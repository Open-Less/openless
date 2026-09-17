# 火山引擎（volcengine）配置

状态：canonical（2026-09-07 以源码为准重写）；更新：2026-09-12。

## 1. 代码中的定义

- Provider：`volcengine`（labelKey `asrVolcengine`），定义于 Core `provider_rules.rs`；`authRequirement = Volcengine`（专用鉴权形态），无内置默认端点/模型（`defaultEndpoint` / `defaultModel` 为空，按通道配置）。
- 验证探针：`asr_silence_allows_no_final`（静音段允许无 final 帧，验证以可取消的静音探测完成）。
- 凭据状态字段（`provider_rules.rs`）：`volcengine_service`（服务选择）、`volcengine_auth_mode`（普通服务的鉴权模式）、`volcengine_app_key`、`volcengine_access_key`、`volcengine_api_key`（凭据是否已配置；具体取值在设置界面录入，凭据走系统安全存储，不落明文）。

## 2. 在应用内配置

设置 → AI 服务与模型 → 语音识别 → 添加渠道，选择火山引擎；按界面提示填入鉴权字段，保存后执行“验证”得到真实验证结果（成功/失败与时间会记录在渠道列表）。

- 服务选择普通服务或 Agent Plan，按渠道保存为 `volcengine.service`（`standard` / `agent_plan`）；旧配置默认普通服务。Agent Plan 使用专属 API Key，不需要 APP ID；切回普通服务保留原鉴权模式及已保存密钥，建议不同服务分别创建渠道。
- Resource ID 留空时使用 `volc.seedasr.sauc.duration`。Coding Plan 不提供 ASR 选项；验证成功后仍需通过实际录音检查转写及插入。

## 3. 端点与排错

- 普通服务的两种鉴权模式均使用 `wss://openspeech.bytedance.com/api/v3/sauc/bigmodel_async`（历史修复 #931 后的行为，以 `crates/openless-core/src/asr/volcengine.rs` 当前实现为准）。
- Agent Plan 使用专属端点 `wss://openspeech.bytedance.com/api/v3/plan/sauc/bigmodel_async` 和 API Key，见[官方接入文档](https://docs.volcengine.com/docs/82379/2516286?lang=zh)。验证与听写共用服务解析，未知服务值报错，不回退到普通端点；连接日志记录端点和追踪 ID，不记录鉴权头。
- 弱网行为：连接超时与重试在 Host/Core 实现，失败信息展示在渠道验证结果中。
- 开通服务、创建应用与获取密钥属火山控制台操作，以[火山官方文档](https://www.volcengine.com/docs)为准；本仓库只维护代码行为。

## 4. 火山方舟语言模型套餐

设置 → AI 服务与模型 → 语言模型 → 添加渠道，选择火山方舟；服务可选普通火山方舟、Agent Plan 或 Coding Plan。

- Agent Plan：`https://ark.cn-beijing.volces.com/api/plan/v3`；Coding Plan：`https://ark.cn-beijing.volces.com/api/coding/v3`。使用各自套餐的专属 API Key，按所选请求格式适配协议路径。
- 选择套餐后，通过“查看支持的模型”打开对应的 [Agent Plan 控制台](https://console.volcengine.com/ark/subscription/agent-plan)或 [Coding Plan 控制台](https://console.volcengine.com/ark/subscription/coding-plan)，复制支持的文本模型 ID，手动填写后执行“验证”。该按钮不拉取在线模型列表，也不验证密钥。
- 预设地址只读，已有自定义地址保持可编辑；新建自定义接口使用自定义供应商入口。普通火山方舟及其他供应商保留原有模型列表获取行为。
