# Android / 鸿蒙输入法后端整理与引擎选型

核对日期：2026-10-17。移动端已确定统一采用 librime，不引入 Fcitx5。Linux 保留现有 Fcitx5 系统宿主。Android 用 JNI、鸿蒙用 Node-API 接到同一套 `mobile-ime/rime_engine.cpp`；两端保留原生键盘和系统输入接口。

鸿蒙已完成 Rime 1.17.0 arm64 构建与签名安装，真机自检通过 `nihao` → “你好”候选、编码退格、候选确认、取消。用户已确认中文候选出现。Android 已改接共用引擎，NDK arm64 原生库及 JNI 编译通过，尚未完成 APK 真机验收和旧学习数据导入。

## 当前问题

鸿蒙早期首版只有英文直输；当前已接 Rime 拼音编码、候选、翻页、空格确认及中英文切换。已接通输入法录音、PCM 输送、ASR/润色管线与结果确认写入，并通过隔离的离线管线检查；真实联网识别尚需配置渠道后验收。

鸿蒙退格使用 IME Kit `InputClient.deleteForward` 删除光标前的内容；`deleteBackward` 删除光标后内容。方向和按键交互已修正，用户确认可删除。诊断仅记录动作类型、返回值和错误码，不记录输入内容；删除键支持长按重复。

移动端现已接入共享 librime，旧 LitePinyin 算法及 TSV 已移除。2026-10-17 核对本机配置发现，桌面默认实际为 Fcitx5 libime 拼音并启用云拼音，而非 Rime。移动端继续使用 Rime，升级固定修订的雾凇中文词库与拼写规则，并接入默认关闭的 Google 云候选；不迁移桌面个人词典，也不引入 Fcitx 到移动端。完整能力及隐私边界见 `openless-all/app/mobile-ime/README.md`。

## 开源候选

| 项目 | 可以复用的部分 | 适配成本及边界 |
| --- | --- | --- |
| [Fcitx5 Android](https://github.com/fcitx5-android/fcitx5-android) / [Fcitx5 Harmony](https://github.com/fcitx-contrib/fcitx5-harmony) | 两个平台的 IME 适配参考 | 已排除，不是当前移动端依赖。 |
| [librime](https://github.com/rime/librime) / [Trime](https://github.com/osfans/trime) | 跨平台 C++ 中文引擎、输入方案、候选和用户词典；Trime 提供 Android JNI 与前端参考 | librime 为 BSD-3-Clause，Trime 为 GPL-3.0。需要编译 Boost、LevelDB、marisa、OpenCC、yaml-cpp 等依赖并适配鸿蒙；目前核对的上游文档未提供可直接装入本项目的 OHOS 构建产物。 |
| [libpinyin](https://github.com/libpinyin/libpinyin) | 拼音算法库 | GPL-3.0；不是完整移动输入法。平台接线、UI 和词库打包仍需自行完成，当前不优先。 |

以上是选型时的比较，最终按用户要求采用 Rime。Fcitx5 / libime 不作为移动端依赖；移动端只复用 librime 及其构建依赖、输入方案和词库。

来源：[鸿蒙子模块清单](https://github.com/fcitx-contrib/fcitx5-harmony/blob/main/.gitmodules)、[鸿蒙按键与提交适配](https://github.com/fcitx-contrib/fcitx5-harmony/blob/main/entry/src/main/ets/InputMethodExtensionAbility/model/KeyboardController.ets)、[librime 构建依赖](https://github.com/rime/librime#build-dependencies)。当前 OpenLess 仓库使用 AGPLv3；是否复用第三方源码或只链接引擎，需按实际文件许可记录，本文未引入第三方源码。

## 共用边界

两端保留原生键盘宿主与渲染，共用引擎接口和行为规范。输入框权限、原生客户端对象和面板生命周期不能抽进跨平台引擎。

1. **引擎适配层（C/C++ + 稳定 C ABI）**：创建/销毁会话、按键、选候选、翻页、清空、方案切换、用户词典。输出编码、预编辑、候选及待提交文字，不接触目标应用。
2. **OpenLess 共用业务层（现有 Rust core）**：语音会话、ASR、润色、风格、渠道、历史。语音最终文字走与候选相同的提交路径；不重写现有提供商逻辑。
3. **Android 平台层（Kotlin / JNI）**：InputMethodService、InputConnection、composing、提交、删除、编辑器 action、权限、原生 UI。
4. **鸿蒙平台层（ArkTS / Node-API）**：InputMethodExtensionAbility、InputClient、Panel、预编辑、提交、删除、编辑器 action、权限、原生 UI。只使用输入法，不接无障碍。

统一请求携带 `sessionId`、`generation` 和递增 `requestId`；异步候选/语音结果必须匹配当前会话才可显示或提交。候选项用稳定 ID，不能只依赖旧列表下标。状态包括语言模式、编码、预编辑、候选及页码、录音状态、错误状态。平台返回提交成功/失败，成功后才清理待提交内容和更新学习记录。

## 行为规范

- 中文字母先进入编码，选择候选或空格确认首选候选；无编码时空格直接插入空格。
- 退格优先删编码；无编码时删选中内容；无选中内容时删光标前一个用户感知字符。平台需处理 emoji、组合字符、目标应用的删除语义，不能统一按一个 UTF-16 单元处理。
- 回车有编码时先确认编码；无编码时按目标编辑器的发送/搜索/换行语义执行。
- 切换输入框、语言、密码框、关闭键盘时明确取消编码与未提交异步结果；不要把旧输入框文本带进新会话。
- 密码框不采集词库学习，不把上下文送给语音/润色服务。普通按键、候选及个人词库默认本地处理。
- 候选区支持翻页；布局区分语言、数字和符号；按键有按下反馈、足够触摸范围、明确删除标签和长按删除。

## 验证与迁移顺序

先验证引擎能在 OHOS arm64 交叉编译，加载真实中文词库，完成 `nihao` → 候选 → `你好` 提交，再验证 Android 使用同一引擎和同一套输入案例。记录离线首启耗时、候选延迟、库与词典体积及生命周期行为。未通过这一步前不整体替换安卓版。

随后接入编码/候选 UI、退格与选区、模式切换、编辑器 action，再接语音结果确认提交。最后逐段迁移安卓版大 Service，保留当前可用功能及用户词库数据，先有稳定新路径再删除旧实现。

共同验收案例包括全拼短句、简拼、重码翻页、连续空格、编码退格、选区删除、中间光标删除、emoji 删除、快速切换输入框、密码框、录音取消、识别结束时切换应用、输入失败后重新确认。两端均使用相同案例，不要求两端 UI 代码相同。
