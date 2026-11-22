# OpenLess 文档入口

## 使用与构建

- [使用指南](../USAGE.zh.md) · [User Guide](../USAGE.md)
- [全平台构建](../openless-all/README.zh.md) · [Cross-platform builds](../openless-all/README.md)
- [Android 平台构建](../openless-all/app/android/README.zh.md) · [Android 输入法与真机验收](android-ime.md)
- [iOS 构建与签名](../openless-all/ios/README.zh.md) · [HarmonyOS 构建与签名](../openless-all/app/harmony/README.zh.md) · [移动端输入法引擎](mobile-ime-engine.md)
- [发布规范](../RELEASING.zh.md) · [Release policy](../RELEASING.md) · [签名索引、镜像与双源更新](release-distribution.md) · [Homebrew 同步与本机提醒](homebrew-openless-updates.md)

## 架构、契约与验收

- [2.0 范围](2.0-requirements.md) · [架构与验证命令](architecture.md) · [源码目录](structure.md) · [客户端图源与导出](images/client-diagrams/README.md) · [桌面验收](2.0-desktop-acceptance.md)
- [CI 触发与缓存](ci-trigger-and-cache-policy.md) · [Apple Silicon DMG 布局](../openless-all/app/src-tauri/dmg/README.zh.md) · [界面滑块](../openless-all/app/docs/sliders.md)
- [加密云同步](encrypted-cloud-sync.md) · [旧版手动快照](cloud-sync.md) · [加密同步条款草案与审查](cloud-sync-agreement/facts-and-legal-review.md)
- [Linux 宿主交接](linux-egui-handoff/README.md) · [后端契约](linux-egui-backend-contract.md) · [缺口和设备证据](linux-egui-handoff/02-gap-register.md) · [日志约定](linux-egui-logging.md)
- [远程输入证书](remote-input-certificates.md) · [录音中断与恢复](remote-input-recording-recovery.md) · [Tauri CSP](tauri-csp.md) · [桌面输入与更新源](desktop-input-and-update.md)

## 语音与服务

- [录音启动与回声消除](audio-startup-latency.md) · [本地 ASR 生命周期](local-asr-lifecycle.md) · [macOS 实验模型](experimental-local-asr.md)
- [渠道选择](provider-channel-selection.md) · [Prompt 缓存](prompt-cache-policy.md) · [DeepSeek 大统一与联网追问](deepseek-suite.md)
- [ASR 结果驱动的润色策略](polish-strategy.md)
- [Gemini 转写](gemini-transcribe.md) · [腾讯混元长语音](tencent-long-audio.md) · [火山引擎](volcengine-setup.md) · [讯飞](xfyun-asr.md) · [百炼](bailian-asr-models.md)
- [感知待办与边界](awareness-follow-up.md) · [用量账本待补范围](usage-accounting-follow-up.md)

## 发布与署名

- [发布记录](releases/) · [组件署名与许可](ATTRIBUTION.md) · [项目许可证](../LICENSE)
- [Windows 现场调查证据](research/)按具体问题与检验基线保留。当前使用和构建步骤以上述指南为准。
