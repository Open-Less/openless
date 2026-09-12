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

设置 → AI 服务 → 文本润色 → 添加火山方舟渠道，在“服务”中选择火山方舟、Agent Plan 或 Coding Plan。选择服务会填入对应 Endpoint，预设地址只读；自定义接口使用现有自定义供应商入口。旧渠道已有的自定义地址保持可编辑，不会自动改写；API Key 和模型沿用现有字段。Agent Plan 与 Coding Plan 均支持语言模型；只有 Agent Plan 提供本页接入的套餐语音识别。

套餐使用各自的专属 API Key 和对应 Endpoint：

- Agent Plan：`https://ark.cn-beijing.volces.com/api/plan/v3`
- Coding Plan：`https://ark.cn-beijing.volces.com/api/coding/v3`

填写控制台显示的**文本生成模型名称**，或使用 `ark-code-latest` 并在控制台选择其对应文本模型，再执行“验证”。不要将图片、视频或向量化模型用于文本润色；列表中的 ID 不代表该套餐均可调用，实际能力以控制台及连接验证为准。

选择 Agent Plan 或 Coding Plan 后，“可用模型”处显示“查看支持的模型”，分别打开对应的[Agent Plan 控制台](https://console.volcengine.com/ark/subscription/agent-plan)或[Coding Plan 控制台](https://console.volcengine.com/ark/subscription/coding-plan)。从控制台复制当前套餐支持的文本模型 ID，填入“模型”字段后验证连接。该按钮不请求在线模型目录，也不验证 API Key。

火山方舟标准服务及其他供应商保留原有模型列表获取行为。套餐的 API Key、模型手填和连接验证沿用现有字段与流程。
