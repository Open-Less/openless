# 旧版手动云快照协议

本页维护 `/me/sync` 兼容客户端和服务端的合同。当前加密同步使用[独立的 `/v1/` 协议](encrypted-cloud-sync.md)。已注册加密恢复门的仓库拒绝旧版多文件恢复，以保护恢复事务。

## 入口与数据边界

[`cloud_sync.rs`](../openless-all/app/crates/openless-core/src/cloud_sync.rs) 从共享 Core 仓储生成快照，使用 `MarketplaceConfig` 的专用云同步地址（生产为 `https://apic.openless.top:9443`，测试与 `new()` 构造默认同源）和同一个 Marketplace 登录实例。GitHub OAuth token 只从 `CredentialStore` 读取，不经过 React，不包含在状态、事件或错误中；注销标记同样对云同步生效。带凭据的请求不跟随重定向，正式地址要求 HTTPS，本机 HTTP 仅用于回环测试。

| 同步内容 | 保持在本机的内容 |
| --- | --- |
| 词典、纠正规则、风格包文本与 PNG 图标 | API 密钥、OAuth token、渠道配置 |
| 主题、胶囊、语言、字号、选中风格、其他列明的个人偏好 | 设备权限、输入/模型路径、Agent 可执行文件/工作目录/权限模式、快捷键与远程输入 PIN |

兼容版快照还同步可迁移的听写与界面偏好，包括光标上下文开关、最近历史窗口、跳过短文本阈值、流式插入、Windows 组合输入、启动最小化、更新渠道、选择语音编辑偏好和布局偏好。光标上下文同步的是开关，不是光标附近的文本；文本仍只在本地隐私门禁通过后按当前设备读取。

具体允许字段由 [`cloud_sync_types.rs`](../openless-all/app/crates/openless-core/src/cloud_sync_types.rs) 定义；不自动映射整个 `UserPreferences`。图标只读取 Core 拥有的资源目录，云端传输的是最多 64 KiB 的 PNG base64，恢复时重新分配本机文件路径。旧风格包的 JPEG/WebP 图标不进入这一版 PNG 同步字段。

## IPC 与冲突处理

Tauri 命令位于 [`commands/cloud_sync.rs`](../openless-all/app/src-tauri/src/commands/cloud_sync.rs)，仅委托 Core：

| 命令 | 参数 | 结果 |
| --- | --- | --- |
| `cloud_sync_status` | 无 | `CloudSyncStatus`：schemaVersion、revision、updatedAt、hasSnapshot、三类数据数量 |
| `cloud_sync_upload` | baseRevision、uiPreferences（locale / fontScale） | 更新后的状态 |
| `cloud_sync_restore` | 无；界面须先确认覆盖本机可同步数据 | status 与 uiPreferences |
| `cloud_sync_delete` | baseRevision；界面须先确认删除云端备份 | 空备份的新状态；本机数据不删除 |

界面只接收数量和版本，不接收完整私有快照。上传与删除使用上次读取的版本，服务端原子比较版本；HTTP 409 转成 `BackendErrorCode::Busy`，`details.reason=revision_conflict`，同时给出 `currentRevision`。客户端不自动改版本重试。上传或删除发生网络错误且无法确认结果时返回 `OutcomeUnknown`，应先刷新状态再决定下一步。

HTTP 请求和存储内容限 2 MiB；输入有条数、文本、PNG 结构及尺寸限制。服务端返回的所有内容均按同一严格 DTO 与限额校验。`translationTargetLanguage` 的空字符串保留“沿用默认输出语言”语义。

## HTTP 合同

官方旧版入口是 `https://apic.openless.top:9443/me/sync`。正式入口使用 TLS，HTTP 仅用于回环测试。请求以 GitHub OAuth token 作为 Bearer 凭据，账户以 GitHub 数字 ID 隔离。客户端设置 30 秒超时并拒绝重定向。PUT 与 DELETE 使用 `application/json`，响应应禁用缓存。

| 方法 | 请求与结果 |
| --- | --- |
| GET | 返回 `schemaVersion: 1`、`revision`、`updatedAt` 和 `payload`。初始状态为 `revision: 0`、`updatedAt: null`、`payload: null`。 |
| PUT | `{schemaVersion: 1, baseRevision, payload}`。仅在 `baseRevision` 等于当前值时原子替换快照、递增版本，并返回保存的快照。`payload` 必须包含 `dictionary`、`corrections`、`stylePacks`、`preferences`。新增偏好字段应兼容旧客户端的缺省值。 |
| DELETE | `{baseRevision}`。版本匹配后将云端 `payload` 置空并递增版本，包括从版本 0 删除的情况。本机数据保留。 |

版本不得超过 JavaScript 安全整数上限 9,007,199,254,740,991。正版本需要可解析的 RFC3339 时间戳。响应体最多 2 MiB 加 1 KiB，字段名采用 camelCase，未知快照字段会被拒绝。词典最多 10,000 条、纠正规则最多 2,000 条、风格包最多 200 个。集合内 ID 唯一，允许 1–128 个 ASCII 字母、数字及 `._-:`，单独的 `.` 或 `..` 无效。图标解码后不超过 64 KiB，宽高各为 1–1024 像素。约束以 [`cloud_sync_validation.rs`](../openless-all/app/crates/openless-core/src/cloud_sync_validation.rs) 为准。

401／403 要求重新登录，404／405／501 表示服务不可用，不能按空备份处理。409 响应提供当前 `revision`，不携带私有快照。其他 HTTP 错误保留本机数据。写请求结果未知时，调用方先重新读取状态再决定后续操作。

## 本地恢复

下载并完整验证后，Core 依次取得设置写入锁、词典锁、纠错锁、风格包锁和偏好锁，生成所有替换文件。凭据和设备字段从当前本机偏好保留；缺少的内置风格包由已有仓储规则补齐。

[`cloud_sync_transaction.rs`](../openless-all/app/crates/openless-core/src/cloud_sync_transaction.rs) 先在目标文件所在目录准备新文件与原文件备份，再依次原子替换。某个写入失败时，以备份反向回滚已经替换的文件；全部文件完成后才更新仓储内存与 PreferencesChanged、VocabularyChanged、StylePacksChanged 事件。若回滚本身失败，保留 `.cloud-sync-*.backup` 并返回 `OutcomeUnknown` 和 `recoveryRequired=true`，不报告成功。

正常完成或成功回滚后删除临时文件。该机制处理进程中可返回的文件错误；它不是 SQLite 式的跨文件崩溃恢复日志。

## 验证

[`cloud_sync_contract.rs`](../openless-all/app/crates/openless-core/tests/cloud_sync_contract.rs) 使用临时 App 数据目录、内存凭据与本机假服务，覆盖同步往返、账号凭据不出界、设备设置保留、版本冲突、删除、错误服务与无效内容，以及恢复准备失败。事务单元测试覆盖中途替换失败后的回滚和回滚失败的真实状态反馈。

在应用目录运行：

```bash
cargo test -p openless-core --locked --test cloud_sync_contract
cargo test -p openless-core --locked --lib cloud_sync_transaction
```

真实服务上线、GitHub 登录和安装包内的设备交互需要另按平台验收；本机假服务测试不代表生产服务已部署。
