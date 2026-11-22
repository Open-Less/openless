# v1 设置扩展与跨版本保留契约

本文是客户端扩展合同及回归入口。同步仍使用原来的客户端加密和生产 v1，不新增服务、文档 kind、密钥通道或同步范围。风格依赖为 `30867f75`、`3330347d`；本批从 `51b65224` 开始，未改应用版本、用户安装、真实数据或生产服务。

## 已核实的服务边界

2026-11-23 只读访问 `https://apic.openless.top:9443/revision` 返回 `09627c2b495ae918d850f459756cb92e986725f1`；`/v1/capabilities` 返回 protocolVersion=1、profile=argon2id-xchacha20poly1305-v1、15 MiB 明文、16 MiB+16 密文、24 MiB HTTP body、7 天幂等收据。指定服务仓库为 `Open-Less/openless-cloud-sync`，本机 `/Users/lvbaiqing/project/openless-cloud-sync`。回归从这个提交的 Git 对象读取服务实现，既不修改它的工作树，也不使用尚未部署的 HEAD 代替生产源码。

| 核查层次 | 真实入口与结果 |
| --- | --- |
| 设置登记 | Core `cloud_sync_e2ee_documents/registry.rs` 决定 Portable / DeviceProfile / Excluded；登记只在客户端生效。 |
| 编码、命名、版本 | `export.rs` 把每个 Portable 设置写成独立的 `(kind=preferences, id=JSON字段名, schemaVersion=1, value=设置值)`。仍为同一个 schemaVersion=1 的 DocumentSet。 |
| 传输 DTO | `cloud_sync_e2ee_protocol/types.rs` 的 SnapshotUpload 只包含加密头、版本、nonce、密文及摘要；不包含设置字段名。`crypto.rs` 先编码并加密整个 DocumentSet，再交给 transport。 |
| 服务校验 | 部署提交 `src/protocol.rs::Snapshot::validate` 检查 envelope 的严格字段、账号、连续版本、KDF/profile/codec、nonce、密文大小与摘要，不解密、不检查 preferences。 |
| 存储和删除 | 部署提交 `src/store.rs::mutate` / `write_vault` 原子替换该账号的完整密文头；没有逐文档 upsert。DELETE 删除整个 vault 的密文并留下递增版本的 deleted 元数据。客户端文档删除由加密 payload 内 tombstone 表达。 |

没有找到“加密 v1 每增加设置就必须更新服务”的源码依据，不能把先前说法作为事实。另一路旧手动 `/me/sync` 的严格 CloudSyncPreferences / payload 合同确实要求两端识别字段，不能把它的约束套在密文 v1 上。真实的新内置包失败点也在旧客户端的 `preflight_native`，并非服务字段白名单。

## 新设置如何加入

1. 先评审范围：可迁移偏好用 Portable；设备路径、平台授权与本机状态按既有 DeviceProfile / Excluded 处理；服务密钥继续使用既有受控 credential accounts。不得把密钥转进 Portable，不新增应用历史、输入内容、瞬态录音任务。
2. 在 `UserPreferences`、对应 Wire 的默认/解码映射和 registry 登记真实的新字段；使用旧版尚未认识的稳定顶层 preferences id。不向 style_packs、channels、device_profile 等旧严格 value 增加字段，不改变旧 id 的类型/枚举含义，不借提示词存设置。
3. 新增可缺省设置用可区分“未设置”和“明确选择默认”的本机表示。此批 `translation_shift_tap_count: Option<NonZeroU8>`：None 不序列化，getter 有效值为 1；Some(1) 必须序列化为 1，允许值 1..255，UI 可以更窄。不能把恢复默认实现为清除字段/设 None。
4. registry 为 `translationShiftTapCount` / Portable / Unsigned8，并拒绝零。界面与手势代码使用 getter 读取、通过原有设置写入入口保存显式值。此提交只供应数据/同步契约，Shift 手势与次数控件由后继任务实现。
5. 为字段增加有效值、排除项、旧版保留、升级物化和显式默认回归。无需服务发布；遇到旧版已经认识且严格限制的旧 id，应使用新的稳定设置 id 和明确的客户端兼容投影，而非改 schemaVersion 或放松生产校验。

未认识的 preferences / ui_preferences 值只在原有受账号、origin、vault、device 绑定的加密 ScopeState 中保留，不散入本机原生设置，也不据此执行任何操作。新客户端首次导出时发现 retained 中已有本版理解的 Portable 设置，会通过现有 `restore_scope` 的完整预检、加密 journal、独占写门、Host 副作用及读回流程物化；已明确存在的本机值优先。随后移除已物化的 retained 副本，避免旧副本重新盖过用户显式选择。服务收到本地物化回执后发布原有恢复事件，使界面重新读取设置。

该通用路径依赖上述“未选择不序列化”的本机表示。把一个已有非可选字段临时改登记为 Portable，并不能区分它的自动缺省与用户选择；这种迁移仍需逐项设计，不能直接删 retained。

## 新内置风格包的兼容实体

旧版 `preflight_native` 要求包的 builtin 类型与其自身内置列表一致。因此把新增的 `openless.intent-reconstruction` 原样以 builtin 上传，旧版会拒绝整个恢复，即使 activeStylePackId 是可解析字符串。

`export_snapshot` 只对旧四个 `builtin.raw/light/structured/formal` 之外的 builtin 包投影为已有的 imported 类型。完整 id、提示词、选区/语音编辑指令、版本、来源、配置及当前选择都保留。旧版拿到的是实际可选、可编辑的包实体，不是孤立引用。

新版 `preflight_native` 调用风格组的 `normalize_synced_builtin_pack`：只有来源两字段、已知发行版本和完整行为配置一致，才把 imported 恢复为本机 builtin；名称或 id 本身不足以升级。支持同源别名 ID；旧版修改 prompt、selectionPrompt 或 voiceEditPrompt 后，保持 imported，沿既有 Store 用户修改保护规则保存，重启不会覆盖。导出仍是 imported，所以原有恢复读回比较与三方合并使用相同 wire 表示。

## 合并、删除与边界

| 边界 | 实际保证 / 限制 |
| --- | --- |
| 整体覆盖 | HTTP PUT 替换整个密文；未知数据的保留必须由参与上传的客户端完成，服务无法替它合并。 |
| 逻辑合并 | preferences 的不同 id 独立三方合并；同一 id 双方修改产生原有脱敏冲突，由用户选择，不能用最后上传覆盖绕过。渠道及其凭据仍为原有合并单元。 |
| 未知值 | 在已测试旧版中，未知 preferences id 的任意受限 JSON 值保留并重新上传，旧版不应用；这不同于允许任意未知文档 kind。 |
| 严格实体 / 未来版本 | 旧版拒绝未知 kind、schemaVersion 或严格实体里的新字段。对此不声称兼容，不把拒绝转换成空快照。 |
| 显式文档删除 | 本机相对共同基线的缺失产生 tombstone；旧标记继续保存，v1 不回收它们。未知设置只因旧版不理解而未落原生文件时，由 retained 提供实体，因此不会误判为删除。 |
| 整库删除 | 版本匹配后删除密文并保留 deleted 版本；读取得到 404/vault_deleted。旧设备自动上传不得复活它；重新创建仍需原有显式用户路径。 |
| 并发 / 丢响应 | 服务 CAS 比较 If-Match / baseRevision，只接受一个头版本；操作 ID 幂等重放使用原字节，原有收据及未知结果恢复机制不变。 |
| 容量 | retained 与 tombstones 同样计入明文 15 MiB、文档及删除标记共 100000、JSON 深度 64；加密按 64 KiB 分帧，密文最多 16777232 字节。不能靠扩展突破这些上限。 |
| 平台 | 恢复继续需要实际平台 Host/credential 能力；缺能力返回错误，不用空设置或假副作用代替成功。测试中的 credential/runtime 是可控夹具，不代表平台钥匙串及设备授权验收。 |

`web_search.config` 属 Application secret account，当前 E2EE 导出只覆盖现有 asr/llm/omni 渠道账号，未包含它。本次保持范围。已发布 v2.0.3 Beta.1 和本批 `51b65224` 的 `WebSearchSection.loadSearchConfig` 重建 provider/apiKey/endpoint 三字段，保存时 JSON.stringify 该对象，会剥掉 providerConfigs；宿主原样存字符串不能保护旧 UI 的这条路径。v2.0.2 stable 尚无该搜索功能，不作为搜索保存测试版本。因此新版 providerConfigs 的本机缓存不能被描述成跨版本云同步，也不能藏入 Portable preferences 绕过密钥边界。

## 回归与版本证据

本次结果：同步相关 211 项测试通过；Clippy `--lib -- -D warnings` 通过；两个已发布旧版共 8 组、包含服务进程重启后原 SQLite 数据库读回的往返通过；历史 v2.0.0 排除项反例按预期拒绝；真实旧搜索 UI codec 的两个扩展丢失反例通过。验证结束后不运行完整发布构建。

运行：

```sh
cd openless-all/app
cargo test --locked -p openless-core --lib cloud_sync_e2ee
cd ../..
python3 scripts/test-sync-v1-released.py --service-repo /Users/lvbaiqing/project/openless-cloud-sync
node scripts/test-sync-v1-search-boundary.mjs
```

runner 从固定 Git 对象读取完整的旧 Core、原 workspace manifest / lock 与必要资源。旧实现没有改 registry、codec、restore、merge 或本机持久化；仅在它已有的测试模块后追加测试入口。服务从固定生产提交提取，追加的测试使用原 router / Store / SQLite 与隔离 OAuth 身份。加解密使用测试固定派生密钥，密码派生仍由已有公开向量测试覆盖。所有合成快照、原始 wire 和临时数据库在 TemporaryDirectory 中，退出后删除；Cargo 缓存在当前工作区忽略的 target 中。搜索边界脚本直接从固定提交抽取原 load/save 函数，仅去除 TypeScript 类型，在模拟 credential IPC 下执行真实逻辑，不改旧函数以伪造保留能力。

| 版本 | 固定源码 | 发布依据 / 验证范围 |
| --- | --- | --- |
| 新客户端 | 本分支，依赖 `3330347d` | 新设置与包投影实现；两份原生仓库、真实加解密、真实文件恢复与重新打开。 |
| v2.0.2 stable | `962705d56984b4c267f450d016aa16b3a881e4b4` | [GitHub Release](https://github.com/Open-Less/openless/releases/tag/v2.0.2-tauri) 于 2026-11-17 UTC 发布；真实旧源码执行保留/恢复/修改/加密上传。 |
| v2.0.3 Beta.1 | `872c3293a8c8b10760017459a280526f3ebdf625` | [GitHub Release](https://github.com/Open-Less/openless/releases/tag/v2.0.3-Beta.1-tauri) 于 2026-11-18 UTC 发布；同一往返路径。 |
| 更早 v2.0.0 tag | `b5e60db6ac010dfc5fdd8aae4db77e1e3a72ac7f` | 作为历史源码反例：cursorContextEnabled 在其 registry 为 Excluded，而当前基线已导出 Portable，旧校验直接 ExcludedField。未找到该 tag 的在线 Release，因此不把它冒充已核实发布版本。 |

兼容下限只承诺本次实际通过的 v2.0.2 stable 与 v2.0.3 Beta.1 组合；没有测试过的更早版本不在承诺内。v2.0.0 的既有分类差异说明“所有 v1 客户端完全向后兼容”不成立，本次不通过静默放宽旧版或生产来伪造兼容。

每个支持的旧版分别覆盖 canonical / 同源 alias × 未编辑 / 编辑三种指令四组：新版选中新包和设置值 2 → 生产同提交服务接收密文 → 旧版解密、恢复、重启、实际解析选择和指令 → 修改旧 theme 设置及可选的包指令 → 旧版合并、加密上传 → 同一服务源码重启并打开原 SQLite 数据库接受回传 → 新版恢复后设置 2 / theme / 包实体与选择均保留。新旧端使用不同 device ID。旧版实际持久化的 preferences、style-packs 和 ScopeState 还会交给新版模拟安装升级，验证保留 2 被物化、不会重复物化、显式改回 1 后另一新版收到 1。未知 preferences 的嵌套值与显式删除标记同样往返。

## 要求、入口、实现与验收对应

以下 Core 路径相对 `openless-all/app/crates/openless-core/`。

| 要求 | 实际入口 | 源码位置 | 验证 / 限制 |
| --- | --- | --- | --- |
| 新设置不依赖服务升级 | 本机设置写入 → 加密同步导出 | `src/shared_types.rs` 的字段/getter；`src/cloud_sync_e2ee_documents/registry.rs`；`export.rs::export_snapshot` | 新客户端原生恢复、getter、显式默认序列化；真实部署提交 router 接受密文。手势/控件由 Shift 后继完成。 |
| 未修改旧版只应用已知数据并保留新值 | 旧版 decrypt → CoreSyncStore → restart → merge → encrypt | `tests/fixtures/v1-released-client.rs`，由 `scripts/test-sync-v1-released.py` 追加到固定旧提交的测试模块 | 实际 v2.0.2 / Beta.1 四组各自验证新值未落旧 preferences.json，但回传仍在。旧代码不修改。 |
| 保留值升级及显式恢复默认 | 新版首次 scope export → 原有 restore_scope | `src/cloud_sync_e2ee_store/mod.rs::export_scope`；`src/cloud_sync_e2ee/service.rs::export_local` | `src/cloud_sync_e2ee_store/tests/v1_compat.rs`：实际旧 ScopeState/native 文件 → 应用 2 → 不重复物化 → 用户显式 1 → 另一新版 1；原有恢复事件只发一次。 |
| 新包选中且旧编辑不丢 | 导出实体投影 → 本机 preflight → Store 重开 | `src/cloud_sync_e2ee_documents/export.rs`；`src/cloud_sync_e2ee_store/native.rs::preflight_native`；风格依赖的 `normalize_synced_builtin_pack` | canonical 和 alias，旧端分别修改三类指令，回到新版保留 imported / 内容 / 当前选择；不是模型效果测试。 |
| 整快照 CAS、重放和删除 | 原部署 router → 原 Store → SQLite 重开 | `tests/fixtures/v1-deployed-server.rs`，引用生产 `09627c2b` 源码 | 版本竞争 412、同操作重放、原数据库读回、DELETE 后 404/vault_deleted。未使用用户账号。 |
| 搜索 secret 边界 | 旧 loadSearchConfig → 编辑 → saveSearchConfig | `scripts/test-sync-v1-search-boundary.mjs`；真实 Beta.1 / 51b65224 UI 函数；Tauri `export_sync_credentials_root` | 两个真实源码 codec 反例；providerConfigs 在旧 UI 保存丢失，Application account 不在当前 E2EE 范围。 |
| 未知种类及协议限制如实报告 | 旧 DocumentSet parser / registry / preflight | 旧版固定源码；本页“合并、删除与边界” | 未知 kind / 严格实体新字段 / 新 builtin 原样发送均有拒绝反例；v2.0.0 另有既有排除项差异，不声称完全向后兼容。 |

该结果不等于已安装旧应用 GUI、真实 OAuth、两台物理设备、系统钥匙串或真实模型输出的验收。包可用的证据是原生仓储实际解析到所选实体及其指令，而非模型效果。

## 与后台录音任务组合时的恢复保护

在集成基线 `504747c8` 上单独核对了录音 A 的 `processingDictations` 拆分。原保护已覆盖后台处理：`api.rs::stop_dictation_session_with_options` 在释放采集后调用 `voice_sessions.submit`；`voice_session.rs::runtime_restore_idle` 同时检查 active 和 submitted，`api.rs` 绑定给 CoreSyncStore 的 probe 使用这个 lease，而非只检查界面采集槽。E2EE 与本地迁移复用同一 Store。

但成功收尾会先 `reset_dictation_session`，再清理语音激活并执行 `persist_completed_dictation`，原 lease 可能在最终历史写入前已经消失。本次只在停止函数封存上下文后增加一个现有 `hold_resources`，将同一 lease 保持到整个收尾函数返回；没有新增状态源，也不改变采集 owner 的释放或后台串行处理规则。

`src/sync_dictation_fence_tests.rs::submitted_dictation_restore_fence_covers_final_history` 使用实际 stop 和 CoreSyncStore 恢复入口，分别验证“采集 Idle、后台任务在途”和“两个界面槽都空、最终历史创建尚未进入仓库写入门”两个时刻均返回 RuntimeBusy，最终历史落盘后 lease 才空闲。第二个窗口通过测试时钟暂停，使用可控引擎，不代表实际麦克风。测试在修复前明确失败，修复后仅运行这一项定向回归；既有 2.0.2 / Beta.1 矩阵不重复运行，也不扩大兼容承诺。

只读 `capture_readonly` 不使用 runtime idle probe，保持原有代次一致性检查；没有把录音处理中一律禁止导出。retained 物化仍由 `export_scope` 调用原 `restore_scope`，因此在同一个 `begin_runtime_restore` 门前被拒绝，不能借“导出”覆盖仍在收尾的数据。selection 与其他组实现未修改。

```sh
cd openless-all/app
cargo test --locked -p openless-core --lib api::tests::submitted_dictation_restore_fence_covers_final_history -- --exact --nocapture
```
