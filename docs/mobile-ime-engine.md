# Android 与 HarmonyOS 输入法接口

Android 通过 JNI、HarmonyOS 通过 Node-API 接入同一个 [`rime_engine.cpp`](../openless-all/app/mobile-ime/rime_engine.cpp) 和 librime。源码构建、拼音方案、候选分页、词库更新和个人学习见[共享引擎指南](../openless-all/app/mobile-ime/README.zh.md)。 Linux 使用自己的 Fcitx5 宿主。

## 平台职责

| 层 | 所有权 |
| --- | --- |
| 共享 Rime 引擎 | 会话、编码、候选、方案和词典。选择候选时校验候选版本。待提交文本保留到宿主明确确认或清除。 |
| OpenLess Core | 语音会话、ASR、润色、渠道和历史。识别结果由对应宿主提交。 |
| Android | Kotlin 管理 `InputMethodService`、`InputConnection`、权限、会话和键盘。见 [Android 源码与构建](../openless-all/app/android/README.zh.md)。 |
| HarmonyOS | ArkTS 管理 `InputMethodExtensionAbility`、`InputClient`、Panel、权限和键盘。见 [HarmonyOS 参考](../openless-all/app/harmony/README.zh.md)。 |

宿主按编辑器代次接纳异步结果。切换编辑器、退出输入法或切换到私密字段时清理旧组合、候选和待提交状态。确认成功后才应答 Rime 的待提交文本。密码框和禁止学习的场景遵循宿主隐私限制，普通按键和用户词库在本机处理。云候选需要额外的显式开启。

HarmonyOS 的 `InputClient.deleteForward` 删除光标前内容，`deleteBackward` 删除光标后内容。录音、识别和输入法生命周期独立于设置 WebView。诊断记录事件种类与错误状态，避免记录编辑器正文。

## 设备验证

两端使用相同的输入用例，以各自系统接口验证。覆盖离线首次启动、`nihao` 候选与提交、翻页、连续空格、编码和选区删除、emoji、快速换框、密码框、录音取消、识别完成前换应用、写入失败和重试。分开记录首启、候选延迟、词库变更后恢复以及安装体积。离线夹具和安装成功不能确认真实网络识别、系统编辑器或权限行为。
