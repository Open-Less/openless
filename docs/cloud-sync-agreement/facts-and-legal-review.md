# 加密云同步事实依据与法律审查清单

> 状态：供开发、运营和法律审查跟进。核对日期 **2026-11-18**；此次客户端源码复核基线 **`a6f4ad5bad93e4c19c8b9acb3369b152c58bfdde`**。此次只读取源码和既有证据资料，不访问真实用户内容、钥匙串、私密 token、生产账号或同步服务接口。下文公开能力、Mivra 与法律资料保留自同日上一轮核对记录；其证据局限仍适用。正式适法结论须由法律顾问审查。

## 1. 已知事实与证据强度

所有源码路径以下列前缀为起点：`openless-all/app/`。原始定位行号来自 `bbf6e7cbaee2a84e3bbeb1fa783675ff8ace7118`；此次复核以新版源码的符号、结构和调用链为准，保留旧行号供追溯。F02、F05、F06、F08、F12 和 F16 补充了此次基线的实现证据。F 类用于说明客户端行为或协议要求；线上部署、安全运营和清理效果仍须取得各自验收证据。

| 编号 | 可支持的陈述 | 源码/来源定位与边界 |
| --- | --- | --- |
| U1 | OpenLess 个人开发者提供、面向全球用户、同步完全免费无费用、开发者及赞助商免费支持；服务器赞助商是 Mivra | 2026-11-18 用户在本任务中直接确认。真实姓名/实体/地址、赞助合同、节点与费用外部边界未核实；赞助关系的依据为用户直接确认 |
| F01 | 新旧同步合同分离；当前加密 origin 和 `/v1/`；自定义服务器也存在 | `crates/openless-core/src/cloud_sync_e2ee/mod.rs:15`；`cloud_sync_e2ee_protocol/transport.rs:145,385,414,437`；`cloud_sync_e2ee/service.rs:740`。仓库 `docs/cloud-sync.md`、`docs/cloud-sync-server-handoff.md` 描述旧 `/me/sync`，不能用其“密钥不上传”等旧范围代替新实现 |
| F02 | 默认关闭；Core 保存范围确认；准备、启用与上传各有完成状态 | `cloud_sync_e2ee/local.rs::ClientSettings`、`dto.rs::CONSENT_VERSION`；`service.rs::prepare_enable/create/unlock/require_consent/run_once`；`src/lib/ipc/cloud-sync-e2ee.ts::CLOUD_SYNC_E2EE_CONSENT_VERSION`；`src/features/cloud-sync/disclosure.json`。此次基线 Core 与 IPC 版本均为 `encrypted-full-snapshot-v2`；准备在连接/刷新后保存本机范围确认，创建成功后才设置 enabled。独立的正式合同接受时间或服务端回执仍待设计与核实 |
| F03 | 11 类逻辑文档：偏好、UI偏好、渠道、登记凭据、词典、词汇预设、纠正规则、风格、历史、活动、设备档案；整体快照非分类选择 | `cloud_sync_e2ee_protocol/types.rs:788`；`cloud_sync_e2ee_documents/export.rs:22`、`types.rs:160,228,263,274`；`cloud_sync_e2ee_store/capture.rs:112`；`cloud_sync_e2ee_documents/registry.rs` 的字段分类和 credential account 列表。API Key 在登记凭据；活动按设备/天记录 count/chars/duration_ms；设备ID/OS/arch/app version在加密DocumentSet内 |
| F04 | 录音不随历史上传，排除本地授权/机密和系统文件，不扫描用户目录 | `cloud_sync_e2ee_documents/export.rs:1,171` 强制 `hasAudioRecording=false`；`registry.rs:86,979` 及 Excluded 字段；`types.rs:274` 明确 coherent capture、非目录/keyring枚举。登记设备路径仅保存路径字段，排除路径所指向的文件内容；`cursorContextEnabled`是开关，实际文本不在此schema |
| F05 | 本地密码NFC、强度与加密，密码/派生密钥不在请求结构中 | `cloud_sync_e2ee_protocol/crypto.rs:21,46,103,133,260,288`；`types.rs:358,428`。12–128字符及ASCII大小写/数字在 `NormalizedPassword::new`；新建/改密额外弱密码检查。协议为Argon2id 64MiB/3次/parallelism4与XChaCha20-Poly1305；此次复核 `crypto.rs::derive_key/encrypt_snapshot/seal_frame` 与上传路径，确认先派生、后认证加密、再交传输层；独立密码学审计尚未核实 |
| F06 | 服务器可见的控制信息与密文；HTTPS/AAD、账号/版本绑定 | `cloud_sync_e2ee_protocol/types.rs:428,522,550,584,664`；`crypto.rs` 的 AAD 和解密校验；`transport.rs:358,469,508,724,814`。明文控制层包括owner/vault/key/operation ID、版本、类型、参数、大小、摘要、服务时间；此次复核 `crypto.rs::EncryptContext::header`、`transport.rs::prepare_upload`：快照正文是密文，控制层与认证另行传输；密文及可关联元数据仍需依法评估 |
| F07 | 记住解锁密钥使用系统凭据库、作用域绑定；本地基线/未定操作加密 | `cloud_sync_e2ee/local.rs:1,110,128`及`remember`/`remembered`/`forget`；`service.rs:1046,1076,1577,2290`。系统本地包装密钥丢失时不静默重建已有恢复资料；锁定写拒绝自动解锁状态后再清理钥匙串。正常应用本机资料仍是应用运行所需的逻辑数据 |
| F08 | GitHub认证与同步密钥分离，发送GitHub bearer换短期会话；自定义token模式另行认证 | `cloud_sync_e2ee/service.rs:754,860,2160,2320`；`cloud_sync_e2ee_protocol/transport.rs:410,432,448`；`types.rs:550`会话有效期1–900秒。账号以数字ID隔离，origin/账号变更清同意；此次复核 `transport.rs::exchange/exchange_with_token`，GitHub bearer 发往 `/v1/auth/github`，静态 token 发往 `/v1/auth/token`。GitHub OAuth使用Marketplace身份适配器；同步文档范围由F03的registry决定。服务器实际日志/令牌保存仍未知 |
| F09 | 已有快照验证后预览；替换/合并及运行态确认；失败回滚与recovery_required | `cloud_sync_e2ee/service.rs:1170,1755,1781`；`cloud_sync_e2ee_documents/restore.rs:168,186`；`cloud_sync_e2ee_store/mod.rs:161,177,186,670`；`store/journal.rs`。预览绑定账号/vault/key/本机代次/远端版本；持久化恢复后不能简单取消，运行态效果失败需回滚；外设备档案不自动执行路径/授予权限 |
| F10 | 改密需要当前密码；新密钥/盐，未知结果谨慎保留旧新绑定 | `cloud_sync_e2ee/service.rs:1996`，先解密校验当前密码再新建密码；没有无旧密码重置接口。设备已解锁时改密仍需当前密码；可取得的本地资料可另行保存 |
| F11 | 自动同步的时序与普通合并 | `cloud_sync_e2ee/service.rs:1912,2366,2410`：启动一次、变更防抖750ms最长5秒；自动路径需启用和密钥，认证由connect检查；本机竞争最多4次有界延后，不做无限网络重试/周期轮询。普通同步可应用无冲突远端变更，冲突须预览选择 |
| F12 | CAS/幂等/收据；超时或404后先核实原操作；已发操作可能继续完成 | `cloud_sync_e2ee_protocol/transport.rs:195,636,684`；`cloud_sync_e2ee/service.rs:1304,1393,1450,1566,1912,2083,2489`。此次复核 `service.rs::run_once/reconcile/require_consent`：新版先检查范围确认，再连接与核对 pending，然后检查 enabled；已发请求可能在关闭后被确认。取消task请求停止；恢复持久化后须完成提交/回滚 |
| F13 | 关闭/锁定/退出/删除不同；删除本地不清空、停用自动上传 | `cloud_sync_e2ee/service.rs:973,1458,1577,1606,2083,2160`。删除要confirmed和vault/版本；退出清认证/密钥，远端注销为有界尽力且可离线；删除状态旧设备须先看见，离线本机不被远程擦除 |
| F14 | 7天是公开声明；备份、收据、删除元数据各自不同 | 2026-11-18只读 `https://apic.openless.top:9443/v1/capabilities` 返回 `protocolVersion=1`、`cryptoProfile=argon2id-xchacha20poly1305-v1`、`maxBackupRetentionDays=7`、`idempotencyRetentionSeconds=604800`。`cloud_sync_e2ee_protocol/types.rs:522`只要求backup天数≤30、幂等保留≥604800秒，`service.rs:860,909`把backup值交UI。协议没有面向用户的历史备份版本选择端点；实操删除与备份恢复策略未知。仓库 `docs/encrypted-cloud-sync.md` 的2026-09-26检查仅历史旁证 |
| F15 | 两类删除标记不能混同，文档墓碑当前不回收 | `cloud_sync_e2ee_protocol/types.rs:584,843`；`cloud_sync_e2ee_documents/merge.rs:214`；`service.rs:973`。文档墓碑在加密DocumentSet内，保存记录身份、时间、基线版本用于防复活；整个vault删除控制元数据在服务器明文层，实际清理期限未核实。7天声明的范围仅涉及服务备份副本；墓碑和其他metadata期限须分别确认 |
| F16 | 服务密钥在客户端服务调用链中使用；同步链路导出登记凭据后加密上传 | 此次复核 `cloud_sync_e2ee_documents/export.rs::export_provider_credentials_for_sync`、`cloud_sync_e2ee_store/capture.rs::capture_native_inner`、`cloud_sync_e2ee/service.rs::create/run_once`；服务调用参考 `cloud_providers.rs::prepare_cloud_transcription/build_cloud_polisher_provider/build_omni_provider`。同步上传结构仅含密文与控制字段，无服务 API 密钥明文或解密密钥；服务 API 密钥与发送到同步认证端点的 GitHub token 分属不同流程。同步服务仅存储传递密文、不使用登记服务密钥调用模型或转写，是须由服务端与运营持续遵守并独立核实的用途限制；本仓客户端源码不覆盖实际服务器的全部行为 |

## 2. Mivra 官网核对（W1）

2026-11-18 使用公开 HTTPS GET 读取首页、`/about.html`、`/contact.html`，不登录、不创建订单、不调用个人接口。来源地址：`https://www.mivra.dev/`、`https://www.mivra.dev/about.html`、`https://www.mivra.dev/contact.html`。

首页将品牌写为 Mivra / 悠雾云数据，介绍云服务器、物理裸机、机柜租用和服务器托管；about 页面介绍基础云计算服务。公开页面中的品牌名和 Organization 标记不能证明注册法律实体、实际合同方或 OpenLess 的部署区域。用户确认“它是服务器赞助商”与官网“它销售何种服务”是两项不同来源的事实。

本次取得的页脚“用户协议”“隐私政策”链接均回到首页；公开 `agreement.htm?id=26/27` 读取仅得到动态模板，没有可核实的完整政策正文。因此未声称已经核对其处理协议、隐私政策、法律实体、资质认证或赞助 SLA；也没有把其官网营销中的全球节点和可用性数字写为 OpenLess 承诺。不能把任何同名开发者工具的数据政策套用于这里的基础设施。

需取得：赞助书/合同及真实提供方、实际服务器与备份所在地区、维护和上游访问、处理指令/保密/删除安排、日志策略、退出与移交机制。不得为核实而向供应商发送真实用户快照、密码或token。

## 3. 官方法律资料与条件性问题

以下均为 **2026-11-18 检索与读取的官方来源**。本清单不是“全球法律已查全”或对所有后续修法的排除证明，也不是法律意见；适用性需按开发者所在地、目标用户、部署及实际处理角色判断。本文不把一次范围勾选等同于所有法律依据，不预选法院或仲裁地。

| 编号 | 官方资料与来源 | 对本草案的有限用途及待审查问题 |
| --- | --- | --- |
| L1 | 国家网信办公开《中华人民共和国个人信息保护法》：`https://www.cac.gov.cn/2021-08/20/c_1631050028355286.htm`，重点第3、13、17、19、21、28–29、38–39、44–50、51、57条 | 如在其适用范围内，应审查处理依据、身份/目的/种类/期限/权利渠道告知、最短必要保留、委托关系、敏感信息及跨境要求。端到端加密不自动证明全部资料脱离个人信息规则；不能泛称接入即同意所有处理 |
| L2 | EUR-Lex，Regulation (EU) 2016/679（GDPR）官方原文：`https://eur-lex.europa.eu/eli/reg/2016/679/oj/eng`，重点Articles 3、6、13、17、20、28、32–33、44起 | 若适用，审查地域范围、各处理目的依据、告知及用户请求、处理者安排、安全/事件通知和跨境。加密是措施之一，不自动等同匿名化。权利范围及例外须依法确定，不能承诺任何情况下都有全部权利或无限期免响应 |
| L3 | 国家网信办《促进和规范数据跨境流动规定》：`https://www.cac.gov.cn/2024-03/22/c_1712776611775634.htm` | 若涉及其适用的跨境活动，再根据实际数据、角色、规模与路径审查所需程序及例外。不得从“免费”“个人开发者”“全球用户”推导自动豁免；具体程序例外的适用条件与持续的信息保护义务须分别核实 |

另外需要当地律师确认：免费服务的合同成立、格式条款提示与说明、责任限制、故意/重大过失和消费者强制性保护；自然人提供者的登记和披露要求；未成年人年龄与监护机制；不同地区监管及请求渠道；服务停用和资料取回的合理通知。**没有在主体和地区未知时断言某国法律已适用，也没有设定零元赔偿上限。** 不因 Mivra 品牌语言、官网备案链接或服务器域名推断 OpenLess 开发者住所或法院管辖。

## 4. 必须确认的事实与法律问题

| 问题 | 当前知道什么 | 发布前应取得的结论/证据 |
| --- | --- | --- |
| 谁对用户负责 | 用户确认个人开发者提供 | 姓名/实体、联系地址、请求与安全渠道；如何在保护个人隐私同时满足披露义务 |
| 全球中的实际地域 | 用户希望面向全球 | 开发者所在地、重点市场、实际受众/未成年人、是否需地域补充、合法可供范围；适用法与争议机制 |
| 资金和赞助 | 免费，开发者与赞助商支持，Mivra是服务器赞助商 | 无费用范围、赞助协议和真实合同方、退出后迁移/关停能力；不许把免费当全免责 |
| 服务器与供应商 | 客户端默认origin；Mivra品牌官网有基础设施服务 | 实际部署/备份位置、上游/CDN、管理权限、处理角色和约束；服务端事实不能由本仓源码替代 |
| 服务端存什么 | wire DTO明确，可见账号/控制资料与密文 | 实际数据库、日志/遥测字段、认证token处理、目的、接收者、期限及清理；现有代码注释不证明运营没有额外数据 |
| 删除7天 | 公开能力声明7天；收据保留至少7天是另一字段 | 备份起算点、清理证据、不可变备份/复制、删除恢复抑制、法律保留；metadata与单文档墓碑的独立安排 |
| 用户请求与离开 | 关闭、锁定、退出、删除快照接口可见 | 账号整体请求、可提供资料、身份核验、处理期限、法定例外、投诉；是否需新增导出/迁移以及安全实现 |
| 安全与事件 | 客户端有密码学、日志脱敏和恢复机制 | 真实运行验证、服务端隔离/最小权限、供应商约束、事件联系人、通知流程及风险处理；不虚称独立审计 |
| 法律接受的证据 | 只有本地同步范围版本字段 | 正式文案版本、语言/可保存性、何时确认、记录主体/位置/期限及最小必要字段；是否另需可撤回的法定同意，不能靠回填版本解决 |
| 责任与中断 | 用户希望尽最大努力保障安全 | 合理注意和具体可履行的措施、双方过错与法定责任、维修/停止通知和取回窗口；避免永久可用/绝对安全或全部损失转嫁 |

## 5. 审查与发布边界

四份 Markdown 文档构成候选正文、用户摘要、开发运营计划及本证据清单，保留用于后续审查。另有 JSON 候选文案供开发告知同步；开发版范围确认版本为 v2，最终法律文本和生效日期待定。运行时文案整合、真实执行验证与 required CI 由各自负责者提供证据。律师批准、服务端部署与删除机制、供应商安排及跨境程序仍待确认。

本次未读取 peer 仓库 `openless-cloud-sync` 的工作树或私有配置。若需继续服务端核实，按项目 cross-project 规则用 Orca 定位和建立独立工作树/责任人，移交精确 `/v1/` DTO、7天backup与≥7天receipt区别、墓碑/删除保留、认证token/日志、供应商地区及所需测试/PR/CI，不假设任何 peer 的状态已绿。服务端和法律待办不得由 UI 的兼容内容加载完成来代替。
