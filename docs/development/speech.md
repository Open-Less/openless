# 语音链路与服务渠道

用户配置见[语音与模型服务](../guides/speech-services.md)。`openless-core` 持有语音会话、渠道解析和各供应商协议。宿主提供真实录音、落字、系统播放及凭据存储。源目录为 [Core](../../openless-all/app/crates/openless-core/src/)。

## 录音与转写

[dictation_engine.rs](../../openless-all/app/crates/openless-core/src/dictation_engine.rs) 在识别器准备之前提交麦克风采集。准备期间，音频写入现有的有界 `BufferedTranscriptionSession`；准备成功后按顺序先发送缓冲，再发送实时音频。稳定模式等录音结束才连接识别器。准备失败、取消及会话终态都要清理已经打开的输入。500 ms 预录仍由独立选项控制。

宿主在音频工作线程运行 DSP。播放参考和麦克风帧由 MicYou 适配层对齐，蓝牙延迟、切换设备、时钟漂移及音量恢复需各自验证。[MicYou 上游与夹具](../../openless-all/app/crates/micyou-dsp/UPSTREAM.md)

## 渠道选择

传统识别和文字处理各自从排序列表选择第一个启用渠道。同一快照解析渠道、模型和凭据；相同排序以 ID 决定。全部禁用时没有可用渠道，只有完全没有渠道配置时才读取旧偏好。旧 active ID 是兼容缓存。多模态与 DeepSeek 大统一分别保留自己的选择和密钥。

服务目录由 `provider_rules::provider_descriptors` 导出。改动 Core 时运行 `cargo run --locked -p openless-core --example export_provider_descriptors` 更新前端目录。火山引擎普通与 Agent Plan 的验证和听写共用地址解析。讯飞用 RTASR 鉴权；百炼按有效模型在经典 WebSocket、Qwen 实时和文件转写之间切换。[服务目录代码](../../openless-all/app/crates/openless-core/src/provider_rules.rs)

使用 ChatGPT 计划润色时，从渠道表单启动登录流程。这个通道持有独立账号凭据。单独的原生和自定义协议仍按各自请求格式解析。

## 特殊服务协议

| 服务 | 请求和完成边界 |
| --- | --- |
| Gemini 专用转写 | 文件模型经 Files API 上传 WAV 并调用 Interactions，成功、失败及取消均走清理。Live 发送 16 kHz 单声道 PCM，显式标记活动开始和结束。只有终稿进入后续链路。传统渠道返回原文，多模态 Smart 在转写调用中整理。专用模型拒绝系统指令及文字任务。 |
| 腾讯混元 Preview | 45 秒后优先在静音处切段，最多 50 秒。每段重新签名、生成 `voice_id`，按实时速率发送排队 PCM。发送结束后收到 `final=1` 才确认一段。后段失败显示整段失败；取消中止握手、收尾与音频队列。 |
| DeepSeek 大统一 | `/v1/tools/asr` 接受录音结束后的 16 kHz WAV。长录音每段至多 120 秒，受 8 MiB 限制。朗读使用宿主 WebSocket，收到成功 `finish` 才算完成。配置独立保存在 `deepseek_suite.config`。 |

纯静音探针可验证协议和凭据是否可用。识别质量、真实日期用量与未付费服务的访问能力需要另行验证。Ask 中的工具调用规则见[随便问接口](ask.md)。

## 提示词缓存与用量

固定指令和可重用前缀放在动态光标、选区和本轮输入之前。Core 按真实 HTTPS 来源、请求协议和模型能力选择支持的缓存参数。自定义网关使用稳定前缀，保留显式路由和缓存配置。路由键是有界哈希。缓存分段拼回去应与原提示词完全相同。

| 服务 | 策略 |
| --- | --- |
| DeepSeek、未知网关 | 稳定前缀，使用供应商已有缓存行为。 |
| OpenAI | Chat Completions 与 Responses 使用稳定路由键。支持显式标记的 Responses 模型另标记静态开发者输入。 |
| Anthropic / OpenRouter | 支持的 Anthropic 模型标记静态系统内容。OpenRouter 在受支持格式保留 `session_id` 和显式标记。 |
| 火山方舟 / 百炼 | 火山方舟仅对支持的 Seed 2 模型添加其路由头。百炼仅对已核实的标准端点及模型添加显式标记。 |

记录用量时保留服务、渠道、模型、协议和返回原值。Chat Completions、Responses 和 Gemini 的输入总量含缓存输入，Messages 的输入字段与缓存读写相加。缓存命中率按同组可比较报告汇总为「缓存读入 ÷ 输入总量」。缺字段保留未知，旧记录不按当前渠道补算。已保存用量与追加覆盖范围分别见[开发需求](../requirements.md#感知表达学习与服务用量)。

回归关注文本与分段恒等、路由参数、静音和重连、取消及用量字段。供应商实测的命中率以返回报告为准。[OpenAI 缓存](https://developers.openai.com/api/docs/guides/prompt-caching) · [Anthropic 缓存](https://platform.claude.com/docs/en/build-with-claude/prompt-caching) · [DeepSeek 缓存](https://api-docs.deepseek.com/guides/kv_cache/)
