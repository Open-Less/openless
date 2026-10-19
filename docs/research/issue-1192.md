# Issue #1192 调研：Android 整套配置本地导入 / 导出

调研日期：2026-10-14  
Issue：<https://github.com/Open-Less/openless/issues/1192>

## 结论摘要

Issue #1192 对 **v1.3.18 稳定版用户**仍然是有效需求，但对当前 `beta` 代码树来说不是从零开始开发：Android IME 已经有一版“导出 / 导入配置”实现，能够通过系统文件选择器迁移一部分本地设置、键盘学习数据、当前 ASR/LLM 渠道和可选 API Key。

这版实现还不能称为“整套配置迁移”。主要缺口是：

- 没有导出完整的 Android / App 偏好；
- 只处理当前激活的 ASR 与 LLM 渠道，不是全部渠道，也没有在不携带凭据时保留完整的非敏感端点 / 模型 / 高级参数；
- 只迁移笔画频率和简拼学习，未迁移 Core 词典、词库预设和完整词库状态；
- 只迁移风格包 ID，不迁移风格包本体、提示词、示例和图标；
- 导出文件是明文 JSON，没有“个人迁移 / 分享配置”两种安全模式、密码加密、导入前备份或可回滚事务；
- 未找到 `OpenLessSettingsExport` 的直接 Kotlin 单元测试或端到端往返测试。

因此建议 **保留 Issue 作为完整迁移的总需求，不直接关闭**，但将现有实现认定为部分交付，并拆出“完整本地 bundle / 安全分享 / 原子导入 / 测试验证”等子任务。若产品只要求“换机时减少重复填写”，当前 beta 版可以作为 MVP；若要求 Issue 正文中的验收标准，则仍需继续实现。

## 一手来源

- Issue 正文与评论：<https://github.com/Open-Less/openless/issues/1192>
- 稳定版 v1.3.18（2026-08-21）：<https://github.com/Open-Less/openless/releases/tag/v1.3.18-tauri>
- 当前 beta 的 Android APK 发布版 v2.0.0-Beta.4（2026-09-30）：<https://github.com/Open-Less/openless/releases/tag/v2.0.0-Beta.4-tauri>
- Android IME 大型 PR #1067（已关闭、未标记为 merged）：<https://github.com/Open-Less/openless/pull/1067>
- Android 凭据安全边界 Issue #817：<https://github.com/Open-Less/openless/issues/817>

## Issue 要求拆解

Issue #1192 要求一个不依赖同一账号、云同步或链接服务的本地文件入口，覆盖：

1. ASR / LLM 及其他服务的协议、渠道名称、Base URL / 完整地址、模型、启用状态、激活渠道和高级参数；
2. 通用词典、用户词条、预设词库、当前激活状态；
3. 风格包本体、提示词、示例及当前默认风格；
4. Android 应用偏好，例如语言、主题、输入与剪贴板策略、触发 / 录音设置；
5. API Key 的显式选择，普通分享默认脱敏；
6. 导入前显示来源、版本、类别和变更摘要，支持合并或替换，并自动备份原配置；
7. 权限不随配置文件迁移，导入后只提示接收者重新授权；
8. 版本化格式、兼容性校验、设备不适用配置的显式跳过或确认。

Issue 的唯一评论来自维护者，内容为“满足你。”；没有关联 PR、标签或进一步的验收讨论。该评论不能替代上述完整验收标准。

## 当前代码现状

### 稳定版与 beta 的边界

在 `v1.3.18-tauri` tag 中不存在 `OpenLessSettingsExport.kt`、Android IME 设置页和 `android_transfer.rs`；这与 Issue 正文以官方 APK 1.3.18 为主要场景的描述一致。当前 `beta` 已包含 Android IME 和导入 / 导出实现，并在 v2.0.0-Beta.4 发布了多架构 APK。

### 已有导出 / 导入入口

[OpenLessSettingsExport.kt](../../openless-all/app/android/kotlin/OpenLessSettingsExport.kt) 定义了版本化 JSON（当前版本为 2），并提供六个可勾选类别：

- `cloudNotes`：云笔记 URL / Token；
- `haptic`：震动开关、强度、时长；
- `strokeFrequency`：笔画输入频率；
- `pinyinLearnedPhrases`：简拼组合学习；
- `providerSelection`：ASR / LLM 当前渠道与两个风格包 ID；
- `credentials`：ASR / LLM 渠道凭据。

入口在 [OpenLessKeyboardSettingsActivity.kt:538-734](../../openless-all/app/android/kotlin/OpenLessKeyboardSettingsActivity.kt#L538-L734)：

- 导出使用 `ACTION_CREATE_DOCUMENT` 写 JSON；
- 导入使用 `ACTION_OPEN_DOCUMENT` 读取 JSON；
- 导入前按文件实际包含的类别弹出复选框；
- 文件读取上限为 4 MiB；
- 导入后显示成功类别与逐项错误。

这版实现已经具备 schema/version、按类别选择、基础输入校验和分项错误报告，不能算完全没有设计。

### Provider / credential 迁移

[android/native_bridge.rs:61-187](../../openless-all/app/src-tauri/src/android/native_bridge.rs#L61-L187) 通过运行中的 Core backend 和 `CredentialsVault` 读写，而不是由 Kotlin 手工修改 `preferences.json`。导入会：

- 校验版本 1 / 2；
- 校验 provider/channel 身份一致性；
- 将渠道合并到目标设备而不是删除其他渠道；
- 刷新 backend 的凭据视图；
- 只在目标设备存在对应风格包时切换风格包，否则保持原值并报告缺失。

[credentials/android_transfer.rs:170-201](../../openless-all/app/src-tauri/src/persistence/credentials/android_transfer.rs#L170-L201) 的导出只取 `root.active.asr` 与 `root.active.llm` 对应的渠道，因此不是所有已配置渠道的完整备份。当前 native bridge 也只处理 ASR、LLM，未覆盖 Omni 渠道。

Android 凭据在设备本地由 Android Keystore + AES-GCM 保护，见 [OpenLessCredentialVault.kt:114-169](../../openless-all/app/android/kotlin/OpenLessCredentialVault.kt#L114-L169) 和 [android_credentials.rs:11-18](../../openless-all/app/src-tauri/src/persistence/android_credentials.rs#L11-L18)。这解决的是“设备存储时”的保护，不等于导出文件安全。

## 与 Issue 验收标准的差异

### 1. 应用偏好覆盖不足

现有 JSON 只读写 `openless_ime_ui` 中的云笔记与震动字段，以及 provider 的几个选择字段。完整的 `UserPreferences` 还包含主题、语言、录音 / 自动停止、剪贴板、流式输入、Android 插入策略、悬浮窗触发模式、手势、尺寸等大量字段，见 [shared_types.rs:385-849](../../openless-all/app/crates/openless-core/src/shared_types.rs#L385-L849)。

Android 自身还把 `theme_mode`、`input_mode`、`latin_input_mode`、英文提示等放在 `openless_ime_ui`，例如 [OpenLessImeService.kt:464-503](../../openless-all/app/android/kotlin/OpenLessImeService.kt#L464-L503)；这些字段不在 `OpenLessSettingsExport.Category` 的导出逻辑中。

**影响：** 当前功能能迁移“键盘设置页里明确列出来的少量选项”，不能满足“安装 APK 后导入即可恢复应用偏好”的目标。

### 2. Provider 的“脱敏分享”不完整

`export_provider_settings_json(false)` 会复制渠道身份后移除 `configuration`，见 [native_bridge.rs:67-79](../../openless-all/app/src-tauri/src/android/native_bridge.rs#L67-L79)。因此不携带 credentials 时，Base URL、模型、协议高级参数等非敏感配置也不会保留；携带 credentials 时则使用完整 `configuration`，其中包含裸 API Key。

此外，导出只取当前激活的 ASR / LLM 渠道，见 [android_transfer.rs:174-188](../../openless-all/app/src-tauri/src/persistence/credentials/android_transfer.rs#L174-L188)，不满足“已配置渠道、启用状态、其他服务均可迁移”的完整范围。

### 3. 词典 / 词库覆盖不足

当前导出只包含 Android 键盘自己的 `StrokeUserFrequency` 与 `LitePinyinLearnedPhrases`。Core 的通用词典和词库预设是独立持久化资源：

- `DictionaryStore` 使用 `dictionary.json`；
- 词库预设使用 `vocab-presets.json`；
- Android mobile command handler 已经注册 `list_vocab`、`add_vocab`、`list_vocab_presets` 等命令，见 [lib.rs:577-588](../../openless-all/app/src-tauri/src/lib.rs#L577-L588)。

因此实现上有现成入口可复用，但当前配置文件没有这些内容，也没有恢复当前启用状态。

### 4. 风格包只迁移引用，不迁移实体

当前 provider selection 只写 `activeStylePackId` 和 `selectionPolishStylePackId`。导入时若目标设备没有对应 ID，则保留旧 ID并报告，见 [native_bridge.rs:146-173](../../openless-all/app/src-tauri/src/android/native_bridge.rs#L146-L173)。这意味着自定义风格的 prompt、examples、元数据和图标不会随配置一起迁移。

Core 已有完整的 ZIP 能力：[persistence/style_pack.rs:112-134](../../openless-all/app/src-tauri/src/persistence/style_pack.rs#L112-L134) 暴露 style pack 的 ZIP 导入 / 导出；Android mobile handler 也已注册对应命令，见 [lib.rs:599-610](../../openless-all/app/src-tauri/src/lib.rs#L599-L610)。后续应把这些资源纳入同一个 bundle，而不是只复制 ID。

实现时需注意 Android SAF 的 `content://` 目标：当前 [commands/style_packs.rs:215-228](../../openless-all/app/src-tauri/src/commands/style_packs.rs#L215-L228) 的 ZIP 导出路径直接按文件路径处理，而导入已经专门支持 `content://`。完整 bundle 需要补充 content URI 写入或在 Kotlin 层统一写出。

### 5. 安全流程与恢复流程不足

导出器明确采用明文 JSON，见 [OpenLessSettingsExport.kt:15-19](../../openless-all/app/android/kotlin/OpenLessSettingsExport.kt#L15-L19)；UI 只展示“文件包含明文 API Key，请妥善保管”的警告，见 [OpenLessKeyboardSettingsActivity.kt:538-555](../../openless-all/app/android/kotlin/OpenLessKeyboardSettingsActivity.kt#L538-L555)。当前没有：

- “个人换机”与“分享配置”两种明确模式；
- 密码加密 / 密钥派生；
- 默认脱敏后仍保留非敏感 provider 配置；
- 导入前快照、自动备份、回滚；
- 合并 / 替换策略选择；
- 写入前的实际变更摘要。

导入是按类别分开写入；非 provider 类别即使前一项成功、后一项失败，也不会进行全局回滚，见 [OpenLessSettingsExport.kt:91-140](../../openless-all/app/android/kotlin/OpenLessSettingsExport.kt#L91-L140)。provider 导入虽然使用 Core 的合并逻辑，但整体配置仍没有统一事务边界。

Issue #817 已明确项目对 Android 凭据的安全要求是 Keystore-backed protection、认证加密、迁移 / 篡改处理和测试；本地导出应遵循同一安全边界，不能因为文件通过系统分享就退回无保护的裸密钥。

### 6. 权限边界目前基本正确，但需要产品化提示

现有导出器没有导出麦克风、悬浮窗、无障碍、输入法启用等系统授权状态；这是正确的，权限不能被配置文件自动授予。完整导入流程仍应在成功后明确列出“尚需在本机授权”的权限，而不是只显示“Imported N categories”。

### 7. 测试覆盖

[android_transfer.rs:281-387](../../openless-all/app/src-tauri/src/persistence/credentials/android_transfer.rs#L281-L387) 已覆盖渠道身份绑定、合并时保留其他渠道、重复导入幂等性；style pack archive / store 也有较完整的 ZIP 安全和持久化测试。

但在当前代码树中没有找到 `OpenLessSettingsExport` 的直接 Kotlin test，也没有覆盖以下完整场景的往返测试：

- 导出所有类别后在干净 Android 数据目录导入；
- 缺失 provider / style pack 的部分成功与回滚；
- 明文 / 脱敏 / 加密三种分享模式；
- SAF `content://` 输入和输出；
- 系统权限不随文件迁移；
- 旧 schema 与未来 schema 的兼容矩阵。

## 与现有 Cloud Sync 的关系

不能简单把用户导向 Cloud Sync：Issue 明确要求无需同一账号、无需云同步的本地文件。但 Cloud Sync 的 E2EE 文档层已经提供了值得复用的“可迁移数据分类”和验证模型：

- [cloud_sync_e2ee_documents/export.rs:27-69](../../openless-all/app/crates/openless-core/src/cloud_sync_e2ee_documents/export.rs#L27-L69) 将偏好分类为 portable / device profile / excluded；
- [export.rs:71-153](../../openless-all/app/crates/openless-core/src/cloud_sync_e2ee_documents/export.rs#L71-L153) 已处理 provider、词典、词库预设、纠错规则和 style packs；
- [types.rs:136-189](../../openless-all/app/crates/openless-core/src/cloud_sync_e2ee_documents/types.rs#L136-L189) 明确区分渠道身份、provider credentials 与完整 style pack；
- [registry.rs:34-83](../../openless-all/app/crates/openless-core/src/cloud_sync_e2ee_documents/registry.rs#L34-L83) 提供字段级 portable / device-bound / excluded 清单。

推荐抽取一个“本地 bundle source / sink”复用这些 canonical record、字段白名单、引用校验和安全错误模型，但不复用账号、服务器或云端同步状态。

## 建议的实现拆分

### P0：先定义稳定本地格式和安全模式

- 统一 ZIP bundle（manifest、schemaVersion、源版本、类别摘要、payload、style assets）；JSON 只作为内部 payload，不作为默认分享格式。
- 明确两种用途：
  - **分享配置**：默认不含凭据，但保留 endpoint / model / protocol / advanced non-secret fields；
  - **个人迁移**：凭据必须单独勾选，并使用用户输入的密码进行 AEAD 加密。
- 统一限制：最大文件大小、最大条目数、压缩炸弹防护、路径穿越防护、未知字段策略、未来版本拒绝策略。

### P1：完整 portable snapshot

- 用 Cloud Sync registry 的 portable / device-bound / excluded 分类为基础，显式定义 Android 可迁移白名单。
- 导出 / 导入完整 `UserPreferences` 中适用于 Android 的字段，以及 `openless_ime_ui` 中的主题、语言、输入模式、键盘偏好和悬浮窗配置。
- 导出所有 ASR / LLM / Omni 渠道的身份与非敏感配置；凭据按渠道和账号单独加密。
- 复用 Core 的 DictionaryStore、vocab presets、correction rules 和 StylePackStore；自定义 style pack 必须携带实体而非只有 ID。

### P1：原子导入与用户可控恢复

- 导入前自动创建本地备份；显示来源、版本、类别、凭据状态和实际变更摘要。
- 支持“合并”与“替换”；默认合并，禁止静默删除目标设备已有渠道 / 词条。
- 用临时目录 / snapshot + 校验后一次性提交；任何类别失败时保留原配置并报告逐项原因。
- 对缺失 provider、风格包、设备相关路径明确标记“跳过 / 需要选择”，不要把 ID 写成不可用状态。
- 完成后提示重启 backend / App，并列出需要用户手动授予的系统权限。

### P2：测试与发布验证

- Kotlin：schema、类别筛选、脱敏、坏文件、版本兼容、大小限制。
- Rust：完整 bundle 的 schema、引用完整性、凭据加密 / 解密、原子提交 / 回滚、style asset 安全。
- Android instrumented：SAF `content://` 读写、Keystore 锁定 / 密钥失效、换机干净安装、旧版本升级。
- 真机矩阵：arm64-v8a、armeabi-v7a、x86_64；覆盖无悬浮窗 / 无障碍 / 未启用 IME 的权限状态。

## 最终 triage 建议

- **不要把 Issue 直接标记为重复并关闭**：稳定版用户仍没有入口，且当前 beta 只完成了子集。
- 可在 Issue 中补充说明：v2 beta 已提供“部分配置导出 / 导入”MVP，并列出上述缺口，避免重复实现已有 provider transfer。
- 将 Issue 标题或描述收敛为“Android 本地完整迁移 bundle（含脱敏分享、可选加密凭据、词典与风格包）”，把当前 `OpenLessSettingsExport` 作为基础实现继续演进。
- 第一阶段优先实现完整非敏感配置、词典 / 词库 / style pack 和备份回滚；凭据分享必须单独经过安全设计与 review。

本报告为代码与 GitHub 一手资料的静态调研，未运行 Android 真机验证或构建 APK。
