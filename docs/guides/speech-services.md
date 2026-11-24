# 设置语音与模型服务

打开「设置 → AI 服务与模型」，先选处理模式。传统模式可分别配置语音识别和文字润色。多模态模式由同一模型处理音频。DeepSeek 大统一模式使用自己的服务密钥和配置。切换模式会保留其他模式的渠道。

## 添加语音识别渠道

在语音识别渠道里添加服务，保存并点「验证」。验证后录一段短音频，检查转写和落字是否符合预期。传统模式使用列表中第一个已启用的渠道。

| 服务 | 要填什么 | 使用提示 |
| --- | --- | --- |
| 火山引擎 | 按所选服务填写鉴权。Agent Plan 用该套餐的 API Key，Resource ID 留空时使用 `volc.seedasr.sauc.duration`。 | 普通服务与 Agent Plan 可以各建一个渠道。Coding Plan 不提供语音识别。 |
| 讯飞实时语音转写 | AppID 和 API Key。 | 确认已开通 RTASR，两项凭据来自同一应用。鉴权失败时也检查系统时间。 |
| 阿里百炼 | DashScope API Key，选择模型。 | 默认使用 `fun-asr-realtime`。自动模式会按模型选择协议，自定义模型可在高级设置里指定。 |
| Google Gemini Transcribe | Gemini API Key，选择 `gemini-3.5-transcribe` 或 `gemini-3.5-transcribe-live`。 | 前者结束后转写，Live 可在录音中显示草稿。启用稳定模式后也会等录音结束才连接。Base URL 可用 `https://generativelanguage.googleapis.com/v1beta`。 |
| 腾讯混元 | 在腾讯云开通实时识别，按渠道表单填写凭据，选择模型。 | `Hy-ASR-3.0-preview` 的长录音会自动分成多个会话。历史录音重转写通常需要接近音频的播放时长。 |

Gemini 的传统渠道返回原文，文字润色用单独的语言模型渠道。用 Gemini 专用转写模型处理多模态音频时，Smart 模式会整理口头重复和自我纠正。需要风格润色、翻译或问答时，使用传统模式加语言模型，或选择支持该任务的多模态模型。

## 配置火山方舟语言模型

在语言模型渠道选择火山方舟，再选择普通服务、Agent Plan 或 Coding Plan。套餐使用各自的 Key。从「查看支持的模型」打开对应控制台，复制模型 ID，填回表单后验证。只有模型支持快速服务档位时才开启「低延迟推理（fast）」。[Agent Plan](https://console.volcengine.com/ark/subscription/agent-plan) · [Coding Plan](https://console.volcengine.com/ark/subscription/coding-plan)

## 使用 DeepSeek 大统一模式

在「AI 服务与模型」选择 DeepSeek 大统一，保存该模式的 API Key。初次配置可以沿用首个已启用的官方 DeepSeek 渠道凭据，保存后单独管理。语音在录音结束后提交。支持的窗口可在回答完成后手动朗读。服务可能计费，以供应商账户为准。[DeepSeek 文档](https://api-docs.deepseek.com/zh-cn/)

更多听写和权限说明见[使用指南](../../USAGE.zh.md)。本地识别模型的安装及限制见[本地模型](../development/local-models.md)。
