# 云同步客户端与兼容协议

加密同步与旧手动快照有各自的命令、HTTP 路由和数据范围。两条路径的覆盖边界在这里集中维护。正式服务条款仍需审查，见[开发需求](../requirements.md#加密云同步正式发布)。

## 加密云同步

协议版本为 `1`，密码学 profile 为 `argon2id-xchacha20poly1305-v1`。本页描述客户端实现和验证边界。服务部署和正式发布状态由各自的发布记录确认。

### 地址与协议边界

客户端默认使用 `https://apic.openless.top:9443/v1/...`。启动同步前读取 `/v1/capabilities`，核对协议、体积上限及配置的 GitHub OAuth client ID，再使用原生层的 GitHub 登录令牌交换短期同步会话。数字 GitHub ID 是账号隔离依据。旧 `cloud_sync_*` 命令继续使用[独立的手动同步合同](#旧版手动快照)。已注册加密同步写入门的仓库拒绝旧 `cloud_sync_restore`，保护加密恢复事务。

2026-09-26 的不带凭据检查中，正式 origin 的 `/v1/capabilities` 返回了协议 `1`、匹配的 OAuth client ID 与加密 profile，备份保留上限为 7 天。这仅证明能力接口当时可用，不代表真实账号授权、私有数据往返或生产验收已经完成。

### 原生边界

- `crates/openless-core/src/cloud_sync_e2ee_protocol/`：严格 wire DTO、规范编码、Argon2id、XChaCha20-Poly1305、带身份与版本的 AAD、HTTPS/CAS/幂等收据。
- `crates/openless-core/src/cloud_sync_e2ee_documents/`：逐字段登记、11 类逻辑文档、渠道和凭据一起合并、删除标记、脱敏冲突与加密恢复日志。
- `crates/openless-core/src/cloud_sync_e2ee_store/`：受控仓库、持久化代次、写隔离、完整恢复与读回验证。
- `crates/openless-core/src/cloud_sync_e2ee/`：默认关闭的状态机、密码解锁、创建、同步、改密、删除、原生凭据库中的记住密钥与加密本地基线。
- `src-tauri/src/commands/cloud_sync_e2ee.rs`：仅主窗口的本地 IPC；`src/lib/ipc/cloud-sync-e2ee.ts` 为对应 typed wrapper。浏览器预览返回 unavailable。
- `src/lib/encryptedSyncUiBridge.ts`：主窗口中实际语言/字号偏好的受控镜像，以及恢复成功后的界面更新。

上传只发送密文。密码和派生密钥不进入服务器请求；系统凭据库只导出登记过的服务账号，不复制 keyring/Keystore 文件。普通服务 API Key、接口设置、文本历史和风格内容纳入加密范围；OAuth 登录态、同步令牌、同步密码/派生密钥、设备私钥、远程输入 PIN、系统权限、录音、模型权重、缓存和程序不进入快照。光标上下文开关本身属于可同步偏好；实际光标文本仍只在开启后按平台隐私门禁读取，不会写入同步文档、历史或日志。

旧版 Omni 初始化可能将默认 `custom` 槽错误保存为空标识。原生读取时仅在目标不存在或规范后全部字段相同的情况下，将这条旧槽完整映射回 `custom`，保留其他已选供应商；不同配置发生碰撞时继续拒绝同步并保留两份数据。读取只规范内存副本，持久化仍走既有写入门，不通过删除凭据或放宽同步文档校验绕过错误。

### 行为

首次开启需明确确认完整范围；已有云端快照必须验证 AEAD 并展示恢复预览，不能用新设备默认值直接覆盖。密码按 NFC 规范化，创建/改密执行本地强度策略。记住密钥由系统安全存储实现，并绑定服务 origin、数字账号、vault、key 和设备。

共同基线、未确认操作及恢复资料在本机加密。网络丢失响应时保留原始请求体和操作 ID，先核对收据，必要时仅重送同一请求。不能将 404 收据当作“未提交”。上传中发生的新本地代次仍待同步。删除状态会停用自动上传，不能由旧设备自动重建。

自动同步仅在已开启、已登录且已解锁时运行：一次启动触发，持久化变更后 750 ms 防抖、最长 5 秒；没有周期轮询或无限重试。关闭与删除是独立操作。跨设备路径和平台设置以设备档案保留，恢复不授予系统权限或自动执行 Agent。

完成有效服务配置后，本机首次引导只申请一次持久化提示。凭据读取失败、配置正在保存、录音/输入/恢复繁忙时不申请；提示不访问同步网络、不启用上传。欢迎流程可直接打开“从云端恢复”，无需先填写服务密钥。窗口隐藏、失焦或任务开始后会丢弃迟到的提示结果。

短暂的本机写入竞争最多触发 4 次有界延后重试；网络失败、结果未知、版本冲突不进入该重试。后续手动任务、锁定或停用会淘汰旧重试链。同步连接遵循当前系统代理偏好，策略变化后重建连接，回环测试始终直连。

测试功能授权 `testingFeaturesEnabled` 不进入远端文档或设备档案；正常恢复和失败回滚均保留接收设备的本机值，不能通过同步打开 Less Computer 或 Claude 控制台。

系统自启动 `launchAtLogin` 不进入远端文档或设备档案；保留接收设备本机值，实际 OS 状态仍由现有 UI 自启动插件管理，恢复不宣称替用户重建自启动授权。

恢复必须绑定 `RestoreRuntimeEffects` 的真实 Host 适配器。所有仓库写入后，在同一加密 journal、独占写入门和运行态屏障内等待绝对目标设置生效；副作用失败按 before 镜像回滚。只保存在本地 journal 的 Agent 开关、provider 镜像和录音标记恢复后，再应用最终旧设置；若回滚副作用也失败，保留 recovery_required，不能提前放行录音或 Agent。未绑定适配器明确失败，不使用空实现代替平台确认。

### 验证边界

协议测试使用公开互操作向量；服务测试使用 loopback 模拟 OAuth/同步端和内存凭据库，不触碰真实账号或用户钥匙串。恢复层须单独覆盖文件/凭据故障、进程中断、账号/版本隔离及排除项。通过协议测试不能替代真实平台钥匙串、两设备恢复与正式 OAuth 的人工验收。

发布遵循 [RELEASING.md](../../RELEASING.md) 的对应提交 CI、签名和资产检查流程。真实 GitHub 授权及两台设备的生产数据往返仍须单独验收，不能由本地模拟服务器的通过结果代替。

## 旧版手动快照

本页维护 `/me/sync` 兼容客户端和服务端的合同。当前加密同步使用[独立的 `/v1/` 协议](#加密云同步)。已注册加密恢复门的仓库拒绝旧版多文件恢复，以保护恢复事务。

### 入口与数据边界

[`cloud_sync.rs`](../../openless-all/app/crates/openless-core/src/cloud_sync.rs) 从共享 Core 仓储生成快照，使用 `MarketplaceConfig` 的专用云同步地址（生产为 `https://apic.openless.top:9443`，测试与 `new()` 构造默认同源）和同一个 Marketplace 登录实例。GitHub OAuth token 只从 `CredentialStore` 读取，不经过 React，不包含在状态、事件或错误中；注销标记同样对云同步生效。带凭据的请求不跟随重定向，正式地址要求 HTTPS，本机 HTTP 仅用于回环测试。

| 同步内容 | 保持在本机的内容 |
| --- | --- |
| 词典、纠正规则、风格包文本与 PNG 图标 | API 密钥、OAuth token、渠道配置 |
| 主题、胶囊、语言、字号、选中风格、其他列明的个人偏好 | 设备权限、输入/模型路径、Agent 可执行文件/工作目录/权限模式、快捷键与远程输入 PIN |

兼容版快照还同步可迁移的听写与界面偏好，包括光标上下文开关、最近历史窗口、跳过短文本阈值、流式插入、Windows 组合输入、启动最小化、更新渠道、选择语音编辑偏好和布局偏好。光标上下文同步的是开关，不是光标附近的文本；文本仍只在本地隐私门禁通过后按当前设备读取。

具体允许字段由 [`cloud_sync_types.rs`](../../openless-all/app/crates/openless-core/src/cloud_sync_types.rs) 定义；不自动映射整个 `UserPreferences`。图标只读取 Core 拥有的资源目录，云端传输的是最多 64 KiB 的 PNG base64，恢复时重新分配本机文件路径。旧风格包的 JPEG/WebP 图标不进入这一版 PNG 同步字段。

### IPC 与冲突处理

Tauri 命令位于 [`commands/cloud_sync.rs`](../../openless-all/app/src-tauri/src/commands/cloud_sync.rs)，仅委托 Core：

| 命令 | 参数 | 结果 |
| --- | --- | --- |
| `cloud_sync_status` | 无 | `CloudSyncStatus`：schemaVersion、revision、updatedAt、hasSnapshot、三类数据数量 |
| `cloud_sync_upload` | baseRevision、uiPreferences（locale / fontScale） | 更新后的状态 |
| `cloud_sync_restore` | 无；界面须先确认覆盖本机可同步数据 | status 与 uiPreferences |
| `cloud_sync_delete` | baseRevision；界面须先确认删除云端备份 | 空备份的新状态；本机数据不删除 |

界面只接收数量和版本，不接收完整私有快照。上传与删除使用上次读取的版本，服务端原子比较版本；HTTP 409 转成 `BackendErrorCode::Busy`，`details.reason=revision_conflict`，同时给出 `currentRevision`。客户端不自动改版本重试。上传或删除发生网络错误且无法确认结果时返回 `OutcomeUnknown`，应先刷新状态再决定下一步。

HTTP 请求和存储内容限 2 MiB；输入有条数、文本、PNG 结构及尺寸限制。服务端返回的所有内容均按同一严格 DTO 与限额校验。`translationTargetLanguage` 的空字符串保留“沿用默认输出语言”语义。

### HTTP 合同

官方旧版入口是 `https://apic.openless.top:9443/me/sync`。正式入口使用 TLS，HTTP 仅用于回环测试。请求以 GitHub OAuth token 作为 Bearer 凭据，账户以 GitHub 数字 ID 隔离。客户端设置 30 秒超时并拒绝重定向。PUT 与 DELETE 使用 `application/json`，响应应禁用缓存。

| 方法 | 请求与结果 |
| --- | --- |
| GET | 返回 `schemaVersion: 1`、`revision`、`updatedAt` 和 `payload`。初始状态为 `revision: 0`、`updatedAt: null`、`payload: null`。 |
| PUT | `{schemaVersion: 1, baseRevision, payload}`。仅在 `baseRevision` 等于当前值时原子替换快照、递增版本，并返回保存的快照。`payload` 必须包含 `dictionary`、`corrections`、`stylePacks`、`preferences`。新增偏好字段应兼容旧客户端的缺省值。 |
| DELETE | `{baseRevision}`。版本匹配后将云端 `payload` 置空并递增版本，包括从版本 0 删除的情况。本机数据保留。 |

版本不得超过 JavaScript 安全整数上限 9,007,199,254,740,991。正版本需要可解析的 RFC3339 时间戳。响应体最多 2 MiB 加 1 KiB，字段名采用 camelCase，未知快照字段会被拒绝。词典最多 10,000 条、纠正规则最多 2,000 条、风格包最多 200 个。集合内 ID 唯一，允许 1–128 个 ASCII 字母、数字及 `._-:`，单独的 `.` 或 `..` 无效。图标解码后不超过 64 KiB，宽高各为 1–1024 像素。约束以 [`cloud_sync_validation.rs`](../../openless-all/app/crates/openless-core/src/cloud_sync_validation.rs) 为准。

401／403 要求重新登录，404／405／501 表示服务不可用，不能按空备份处理。409 响应提供当前 `revision`，不携带私有快照。其他 HTTP 错误保留本机数据。写请求结果未知时，调用方先重新读取状态再决定后续操作。

### 本地恢复

下载并完整验证后，Core 依次取得设置写入锁、词典锁、纠错锁、风格包锁和偏好锁，生成所有替换文件。凭据和设备字段从当前本机偏好保留；缺少的内置风格包由已有仓储规则补齐。

[`cloud_sync_transaction.rs`](../../openless-all/app/crates/openless-core/src/cloud_sync_transaction.rs) 先在目标文件所在目录准备新文件与原文件备份，再依次原子替换。某个写入失败时，以备份反向回滚已经替换的文件；全部文件完成后才更新仓储内存与 PreferencesChanged、VocabularyChanged、StylePacksChanged 事件。若回滚本身失败，保留 `.cloud-sync-*.backup` 并返回 `OutcomeUnknown` 和 `recoveryRequired=true`，不报告成功。

正常完成或成功回滚后删除临时文件。该机制处理进程中可返回的文件错误；它不是 SQLite 式的跨文件崩溃恢复日志。

### 验证

[`cloud_sync_contract.rs`](../../openless-all/app/crates/openless-core/tests/cloud_sync_contract.rs) 使用临时 App 数据目录、内存凭据与本机假服务，覆盖同步往返、账号凭据不出界、设备设置保留、版本冲突、删除、错误服务与无效内容，以及恢复准备失败。事务单元测试覆盖中途替换失败后的回滚和回滚失败的真实状态反馈。

在应用目录运行：

```bash
cargo test -p openless-core --locked --test cloud_sync_contract
cargo test -p openless-core --locked --lib cloud_sync_transaction
```

真实服务上线、GitHub 登录和安装包内的设备交互需要另按平台验收；本机假服务测试不代表生产服务已部署。
