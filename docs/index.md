# OpenLess 文档总目录

从你的目标直接进入对应文档。面向用户的在线手册持续维护于 **OpenLess 官网文档**：<https://openless.top/zh/docs/>（English: <https://openless.top/en/docs/>）。

## 上手与使用 OpenLess

| 想做什么 | 文档入口 |
| --- | --- |
| 了解 OpenLess、平台支持、功能亮点与路线图 | [中文 README](../README.zh.md) · [English README](../README.md) · [官网文档](https://openless.top/zh/docs/) |
| 下载各平台安装包与查看更新记录 | [官网下载与安装](https://openless.top/zh/docs/installation/) · [GitHub Releases](https://github.com/Open-Less/openless/releases) |
| 安装、授予系统权限、听写、风格包、词典与云同步 | [中文使用指南](../USAGE.zh.md) · [English User Guide](../USAGE.md) |
| 配置语音识别（ASR）、语言模型（LLM）、多模态或 DeepSeek 大统一 | [官网配置入门](https://openless.top/zh/docs/setup/) · [仓库入口说明](guides/speech-services.md) |
| 查看润色、口述改口与 Prompt 结构化参考样例 | [润色参考样例（JSONL）](references/polish-reference-examples.sample.jsonl) · [官网语音输入与润色](https://openless.top/zh/docs/voice-input/) |
| 用手机浏览器录音并把文字输入到电脑 | [手机远程输入指南](guides/remote-input.md) |
| 为 macOS Homebrew 安装的 OpenLess 开启更新提醒 | [Homebrew 更新提醒](guides/homebrew.md) |

## 开发、构建与架构

先阅读 [架构与模块说明](architecture.md) 了解三层边界与数据流，再按 [开发者全平台构建指南](../openless-all/README.zh.md) 搭建本地环境。

| 主题 | 文档入口 |
| --- | --- |
| 三层架构、全平台客户端形态、验收进度与模块/子文档索引 | [架构、全平台形态与模块索引](architecture.md) · [客户端架构图与 Mermaid 源码](images/client-diagrams/README.md) |
| 开发者全平台构建指南（macOS / Windows / Linux / Android / HarmonyOS / iOS） | [中文全平台构建指南](../openless-all/README.zh.md) · [English Build Guide](../openless-all/README.md) |
| Linux 原生 egui 宿主、接口契约与命令事件基线 | [Linux 宿主与 egui 接口说明](development/linux.md) · [命令与事件兼容基线 JSON](linux-egui-command-event-baseline.json) |
| iOS 原生应用、键盘扩展、构建与签名指南 | [iOS 中文构建指南](../openless-all/ios/README.zh.md) · [iOS English Guide](../openless-all/ios/README.md) |
| Android 构建与输入法说明 | [Android 中文说明](../openless-all/app/android/README.zh.md) · [Android English Guide](../openless-all/app/android/README.md) |
| HarmonyOS 构建与签名说明 | [HarmonyOS 中文说明](../openless-all/app/harmony/README.zh.md) · [HarmonyOS English Guide](../openless-all/app/harmony/README.md) |
| 移动端共享输入法核心（Android / HarmonyOS） | [Mobile IME 中文说明](../openless-all/app/mobile-ime/README.zh.md) · [Mobile IME English](../openless-all/app/mobile-ime/README.md) |

## 发布、署名与许可

| 主题 | 文档入口 |
| --- | --- |
| 维护者发布流程、版本同步闸门、签名与更新清单 | [中文发布清单](../RELEASING.zh.md) · [English Release Checklist](../RELEASING.md) |
| 第三方 UI 组件与 Open Runde 字体署名 | [组件与字体署名说明](ATTRIBUTION.md) |
| 开源许可证（OpenLess 2.0 为 AGPL-3.0-only；已发布 1.x 为 MIT） | [LICENSE](../LICENSE) |
