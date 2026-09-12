# 火山引擎（Volcengine）配置

## ASR 服务与鉴权

设置 → AI 服务 → 语音识别 → 添加渠道，选择火山引擎。服务选择与普通服务的鉴权模式独立，配置和密钥按渠道保存至系统凭据存储。

| 服务 | 鉴权 | WebSocket 端点 |
| --- | --- | --- |
| 普通服务 | APP ID + Access Token，或普通语音控制台 API Key | `wss://openspeech.bytedance.com/api/v3/sauc/bigmodel_async` |
| Agent Plan | Agent Plan 专属 API Key，不需要 APP ID | `wss://openspeech.bytedance.com/api/v3/plan/sauc/bigmodel_async` |

未包含服务字段的旧配置继续使用普通服务。Agent Plan 自动使用 API Key 鉴权，切回普通服务后保留原有鉴权模式；切换服务不会删除已保存的密钥。不同服务使用不同密钥，建议分别创建渠道。Coding Plan 没有 ASR 服务选项。

Resource ID 留空时使用 `volc.seedasr.sauc.duration`；Agent Plan 豆包流式 ASR 使用此资源。服务选择保存在 `volcengine.service`（`standard` / `agent_plan`），由 Core 的同一配置解析和连接路径用于验证、听写及其他 ASR 入口。未知服务值报错，不回退到普通计费端点。

“验证”发送可取消的静音探针，允许没有最终识别结果；连接验证成功后，还应通过实际录音检查转写及插入。连接日志包含端点、连接/请求 ID 和服务端 Log ID，不包含鉴权头。

官方依据：[Agent Plan 接入语音模型](https://docs.volcengine.com/docs/82379/2516286?lang=zh)、[普通流式语音识别](https://www.volcengine.com/docs/6561/1354869?lang=zh)。

## Ark 语言模型套餐

设置 → AI 服务 → 文本润色 → 添加火山方舟渠道，使用套餐专属 API Key 和对应 Endpoint：

- Agent Plan：`https://ark.cn-beijing.volces.com/api/plan/v3`
- Coding Plan：`https://ark.cn-beijing.volces.com/api/coding/v3`

填写控制台显示的**文本生成模型名称**，或使用 `ark-code-latest` 并在控制台选择其对应文本模型，再执行“验证”。不要将图片、视频或向量化模型用于文本润色；列表中的 ID 不代表该套餐均可调用，实际能力以控制台及连接验证为准。

模型列表与推理验证相互独立。目录请求返回 404 时，应用提示当前 Endpoint 无法提供列表，保留已有模型和手填入口，不将目录缺失判定为 API Key 无效，也不会替换 Endpoint。Coding Plan 的现有 `/models` 路径继续使用。

Agent Plan 的官方目录接口 [ListArkAgentPlanModel](https://api.volcengine.com/api-docs/view?action=ListArkAgentPlanModel&version=2024-01-01&serviceCode=ark) 使用独立签名鉴权，应用没有接入这一管控接口；它与推理 Key 的 `/models` 请求不同。手填模型配置见[官方快速开始](https://docs.volcengine.com/docs/82379/2373738?lang=zh)。

润色请求保留现有可选温度规则：发送配置值的十进制表示（例如默认 `0.3`），避免将 `f32` 扩展为长小数。自定义渠道未配置温度时仍省略该字段，OpenAI GPT-5 和各协议的既有省略规则保持不变。
