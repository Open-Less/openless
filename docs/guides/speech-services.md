# 语音与模型服务配置指南 / Speech & Model Services Setup

完整的语音识别（ASR）、文字处理（LLM）、多模态听写与 DeepSeek 大统一配置教程、服务商步骤与排查说明已统一维护在 **OpenLess 官方文档站**：

- **中文配置文档**：<https://openless.top/zh/docs/setup/>
- **English Setup Guide**：<https://openless.top/en/docs/setup/>

## 快速入口 / Quick Links

打开 OpenLess「设置 → AI 服务与模型」，根据你的使用习惯任选一条识别管线即可：

| 管线 / Pipeline | 说明 / Overview | 官网直达 / Official Docs |
| --- | --- | --- |
| **传统模式 · ASR（语音识别）** | 火山引擎、阿里云百炼、讯飞实时语音转写等，将语音转写为原文。 | [配置你的 ASR](https://openless.top/zh/docs/setup/#配置你的-asr) · [Configure ASR](https://openless.top/en/docs/setup/#configure-asr) |
| **传统模式 · LLM（文字润色）** | DeepSeek、火山方舟等语言模型，按风格包去口癖、改口修正、润色或翻译。 | [配置你的 LLM](https://openless.top/zh/docs/setup/#配置你的-llm) · [Configure LLM](https://openless.top/en/docs/setup/#configure-llm) |
| **多模态模式（单模型听写）** | Google Gemini、OpenAI Audio、阿里云 Qwen-Omni 等支持音频输入的模型一步完成转写与整理。 | [多模态听写](https://openless.top/zh/docs/setup/#多模态) · [Multimodal](https://openless.top/en/docs/setup/#multimodal) |
| **DeepSeek 大统一** | 使用 DeepSeek 官方 API Key 统一承接语音听写、问答、联网搜索与语音朗读。 | [DeepSeek 大统一](https://openless.top/zh/docs/setup/#deepseek-suite) · [DeepSeek suite](https://openless.top/en/docs/setup/#deepseek-suite) |
| **试录与验收** | 保存并验证渠道后，在空白输入框中试录一句话确认转写与光标落字。 | [试录一句](https://openless.top/zh/docs/setup/#试录一句) · [Try a short recording](https://openless.top/en/docs/setup/#test-dictation) |

## 安全与延伸阅读

- **凭据存储**：桌面端与移动端的 API Key 保存在操作系统安全凭据库（macOS Keychain / Windows Credential Manager / Linux Secret Service / iOS Keychain）中，不会写入代码仓库。请勿在 Issue、日志或截图中暴露密钥。
- **使用说明**：安装、系统权限、快捷键、风格包与个人词典见 [中文使用指南](../../USAGE.zh.md) · [English User Guide](../../USAGE.md)。
- **润色与改写参考样例**：见 [polish-reference-examples.sample.jsonl](../references/polish-reference-examples.sample.jsonl) 与官网 [语音输入与润色](https://openless.top/zh/docs/voice-input/)。
