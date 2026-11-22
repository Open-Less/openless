# OpenLess 文档

从你要做的事选一个入口。

## 使用 OpenLess

| 想做什么 | 阅读 |
| --- | --- |
| 下载应用或看某个版本改了什么 | [GitHub Releases](https://github.com/Open-Less/openless/releases) |
| 安装、授予权限、听写、使用词库或云同步 | [中文使用指南](../USAGE.zh.md) · [English user guide](../USAGE.md) |
| 配置语音识别或语言模型 | [语音与模型服务设置](guides/speech-services.md) |
| 用手机录音并把文字输入电脑 | [手机远程输入](guides/remote-input.md) |
| 给 Homebrew 安装的 OpenLess 设置更新提示 | [Homebrew 更新提醒](guides/homebrew.md) |

## 开发 OpenLess

先读[架构与接口调用](architecture.md)定位模块，再按[全平台构建指南](../openless-all/README.zh.md)配置开发环境。

- **听写与模型**：[语音链路与服务渠道](development/speech.md) · [本地模型管理](development/local-models.md)
- **文字处理**：[问答与选区调用](architecture.md#问答与选区) · [转写后的自定义润色策略](development/polish-strategy.md) · [按输入应用选择风格包](development/app-style-routing.md)
- **数据与安全**：[云同步与旧快照协议](development/cloud-sync.md) · [远程输入协议](development/remote-input.md) · [WebView 内容安全策略](development/webview-security.md)
- **宿主与排查**：[Linux 宿主接口与日志](development/linux.md) · [Windows 无响应诊断](development/windows-diagnostics.md)
- **构建与发布**：[发布规范](../RELEASING.zh.md) · [自动检查、签名与分发](development/delivery.md)
- **维护资料**：[架构图图源](images/client-diagrams/README.md) · [AOCI Code 索引](aoci-code.md) · [第三方组件署名与许可](ATTRIBUTION.md)

## 查未完成工作

[开发需求](requirements.md)汇总仍需实现、确认或真机验收的工作。云同步的[协议草案](cloud-sync-agreement/agreement-draft.zh-CN.md)、[摘要草案](cloud-sync-agreement/user-summary.zh-CN.md)和[事实依据](cloud-sync-agreement/facts-and-legal-review.md)供发布前审查。
