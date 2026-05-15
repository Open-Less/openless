# OpenLess Android 商店提交流程清单

## 应用标识

- 最终包名已确认
- 最终应用名称已确认
- 最终 `versionCode` 已确认
- 最终 `versionName` 已确认
- Release 签名 keystore 与 alias 已确认

## 商店素材

- 应用图标已导出为商店要求的尺寸
- 如目标商店需要，Feature Graphic 已准备
- 手机截图已采集
- 如声明支持平板，平板截图已采集
- 短描述已撰写
- 完整描述已撰写

## 政策与披露

- 麦克风用途披露已准备
- 悬浮窗/浮窗用途披露已准备
- 剪贴板行为披露已准备
- 网络/Provider 配置披露已准备
- 隐私政策 URL 已准备
- Data Safety / 隐私问卷已准备

## 功能验证

- 已在至少一台真机上完成 `QA_CHECKLIST.md`
- 已验证当前 Android 目标版本下的悬浮触发器
- 已验证当前 Android 目标版本下的 IME 直接插入
- 已验证剪贴板兜底
- 已验证问答文本输入流程
- 已验证问答语音流程
- 已在至少一个支持应用中验证选中文本 `PROCESS_TEXT` 流程
- 已验证翻译流程
- 已用真实凭据验证火山 ASR 流程
- 已用真实凭据验证 Whisper 兼容流程

## 打包验证

- 已完成 `.\build.ps1 -Configuration release ...`
- 已完成 `.\verify.ps1 -Configuration release`
- 已归档 APK/制品与对应发布说明
- 已复核 Manifest 标签与所有可见文案

## 已知限制披露

- Android 端使用“悬浮窗 + IME”等价机制，而不是桌面端全局热键直接插入
- 选中文本问答依赖目标应用是否支持 Android 文本操作
- 直接插入依赖 OpenLess 输入法处于激活状态
- 提供商诊断不能替代完整真机端到端验证
