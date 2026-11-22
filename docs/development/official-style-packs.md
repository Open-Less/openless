# 官方风格包、整体意图与口述修正

本次从 local/beta 的 `51b65224` 实现，不修改应用版本、安装中的应用或真实用户数据。

## 来源与实际路径

只读读取本机 `~/Library/Application Support/OpenLess/style-packs.json`，找到「专为构建而生」：本地 ID `openless.intent-reconstruction`，作者 `OpenLess,TRIP`，版本 `1.0.0`，市场来源 `eb6a236e-3bc3-4a04-8168-851bdf797230`，来源作者 `appergb`，类型 imported、基础模式 structured，无推荐模型或图标，voiceEditPrompt 为空。本机记录创建于 2026-11-22 07:37:04 UTC，更新于同日 08:51:51 UTC。完整指令、选区指令、示例、标签及配置保存在 Core 的 `src/builtin_styles/intent-reconstruction-v1.json`，作为发行来源和安全升级的比较基线；去掉了机器时间戳和运行时 active 状态。

只读检索 NextTodo「试用风格包」并读取 #23「制作并发布全局意图重构试用风格包」；事项已完成，描述列出了原有试用验收，但没有提供模型请求或输出证据。因此不能把该事项勾选状态作为本次模型效果验证，也没有改动事项。

正常听写入口：`OpenLessBackend::capture_dictation_context` 解析所选包，`DictationContext::capture` 冻结 `dictation_style_prompt(pack)`。传统 ASR 后润色经 `cloud_providers.rs` 的润色分发调用 OpenAI/Gemini 共用的 `prompt_compose`；组合翻译使用现有源文和译文标记协议。多模态经同文件 `build_omni_prompt`，从冻结的 context 构建相同系统提示词，把完整音频送入 Omni provider；多模态翻译仍沿用现有仅输出译文规则，不增加第二套组合路径。选区编辑继续读取 selectionPrompt；voiceEditPrompt 为空时沿用原有默认。

## 改了哪些包

| 包 | 内置版本 | 原因 |
| --- | --- | --- |
| 专为构建而生 | 1.0.0 → 1.1.0 | 纳入现有内置分发；保留真实源包的整体理解、信息融合、术语和边界规则，把强制五层及空层改为按实际内容组织，加入统一口述修正规则。 |
| 清晰结构 | 3.0.0 → 3.1.0 | 加强只整理不回答、完整阅读后组织、扩大范围更新整体目标、保留细节和未决问题；加入口述修正规则。 |
| 轻度润色 | 2.0.0 → 2.1.0 | 加强任务边界、完整信息和语气保护，明确原意优先于旧字数比例；加入口述修正规则。 |
| 正式表达 | 2.0.0 → 2.1.0 | 同上，尤其保护不确定程度和请求目的，避免正式化变成代答或补充方案。 |
| 原文 | 保持 1.0.0 | 保持原文直通及取消翻译后不请求模型。 |

新指令针对已观察到的 DeepSeek 遵循问题加强约束，没有增加审稿、重试或模型调用。尚无本次真实 DeepSeek 输出证据，不能宣称遵循率提高。

## 迁移、身份与兼容

`style_pack_store::reconcile_builtin_packs` 复用现有内置版本更新。旧官方三包只有提示词全文等于随源码保留的已知发行文本才自动升级；用户修改的提示词及无法证明来源的历史文本不覆盖。普通官方包只更新 dictation prompt 和版本，保留用户的选区、语音编辑、名称、启用状态和其他配置。

新官方包采用原本地 ID 和市场来源。对 ID 不同的已安装同源包，必须同时匹配 originPackId 与 originAuthorLogin，并逐项比较 prompt、selectionPrompt、voiceEditPrompt、baseMode、examples、recommendedModel 后，才认定等价并原位接管升级；保留原 ID、名称、启用状态、当前选择和创建时间，不改分类/应用绑定和历史引用。名称相同不足以接管。来源相同但内容修改的不同 ID 包保留为独立用户版本，官方包另行提供；占用官方 ID 但内容修改的包也不覆盖。不会删除或重映射既有引用。

只给四个历史模式迁移 preferences 的旧提示词；按明确 builtin ID 同步各模式偏好，避免新增 structured 模式的包抢占旧清晰结构。离线新装增加第五包，旧用户原选择不变；重复启动不重复写入；旧快照经现有 normalize 路径重新补齐并保留别名 ID。

没有新字段、枚举或同步文档类型，但仅能解码 JSON 并不足以证明跨版本可恢复：同步组已用真实 v2.0.0/v2.0.2 确认，native preflight 会拒绝旧四个内置 ID 之外的 builtin，导致整次恢复失败。同步组负责在 v1 导出将新 builtin 投影为 imported，保留完整实体和引用；新版 native preflight 在身份检查前调用 style_packs::normalize_synced_builtin_pack，只在来源、版本和行为配置等价时恢复 builtin，旧客户端改过的内容保持 imported。此纯函数不改 ID、提示词或选择，来源别名也保留原 ID。同步组还必须允许新官方 ID 被用户编辑后继续作为 imported，不能仅凭 ID 强制内置身份。本风格提交仅提供 helper 与 fixture，不修改同步模块；完整跨版本往返须由同步组接入验证。

## 场景合成契约

`style_packs::dictation_style_prompt(&StylePack)` 是唯一风格及非内置尾缀入口。内置身份要求 kind、稳定 ID 或来源身份及随发行提供的提示词一致，不能只看包名；已修改的内置包按自定义请求处理。内置修正规则保存在包内，非内置在请求时追加一次，绝不写回用户内容。若用户提示词已含完整相同规则，则不重复追加。规则只确定素材里的有效意思，不覆盖固定输出、风格格式或翻译协议。

场景实现现已单独冻结场景前缀（见[冻结场景契约](app-style-routing.md#冻结应用场景前缀)），在最终请求合成时形成：场景前缀 → 风格 → 仅非内置口述修正尾缀 → 本次完整输入。不得将场景前缀写回用于 Raw 身份判断的 canonical style_system_prompt。风格组件负责 `dictation_context.rs` 的风格初始化；场景组件负责独立前缀采集及请求时前置，不写回 canonical 风格文本。

## 要求、入口、位置和验证

源码路径以下相对 `openless-all/app/crates/openless-core/`。

| 要求 | 实际入口 | 源码位置 | 验证与限制 |
| --- | --- | --- | --- |
| 新装、升级、重复启动 | StylePackStore 启动 | src/style_packs.rs 的 builtin_intent_pack；src/style_pack_store.rs 的 reconcile_builtin_packs | style_pack 测试 42 项及新增 offline_install 测试 1 项通过；只在测试临时目录运行 Store。 |
| 同源等价去重、用户修改保护 | 同一启动/快照归一化路径 | src/style_pack_store_tests.rs 的 official_intent_adoption、modified_and_namesake、newly_bundled | 验证别名 ID、版本、禁用/active、reset、旧快照、所选听写和选区包保留；不改绑定及历史 ID。 |
| 反悔规则一次、完整输入与翻译 | effective_polish_prompts | tests/prompt_contract.rs 的 dictation_revision_contract | 7 项通过；八类输入 × 五个包 × 润色/翻译；证明输入完整和规则只出现一次，不能证明语义效果。 |
| 正常听写请求和 Raw 保护 | start_dictation → ASR → SharedCloudTextPolisher → HTTP | tests/dictation_target_capture.rs 的 routed_dictation_sends | 4 项通过；内置四个润色包、自定义、翻译、固定输出夹具；原文零请求、开启翻译后取消（实时/提交时）零请求通过。 |
| 多模态使用路径 | Omni 完整音频请求 | src/cloud_providers.rs 的 build_omni_prompt | 只读源码核对复用同一冻结提示词；本次未运行真实音频或真实 Omni 模型验证。 |
| 真实 DeepSeek 输出 | 用户现有渠道 | macOS 系统钥匙串凭据入口 | 安全进程内读取 credentials.v2 在 15 秒后超时，无凭据导出。只读 preferences 显示 activeLlmProvider 为 ark，但具体 DeepSeek 渠道和模型名无法核实，因此未发送真实请求。 |

八类输入：官网文档后扩大首页视频和文字（保留不跳转、保持展示、问句）、周三改周四下午、周三或周四、明确撤回、简单日常、问句、未决冲突、长输入。验收真实输出时仍须逐项确认最终整体范围、唯一有效时间、并列选项、其他信息保留及问题仍为问题；不能将 HTTP 夹具返回的固定文字当作通过。

下一步由协调者接入后继场景任务，并在系统凭据可读时使用用户实际 DeepSeek 渠道补足真实模型语义验收；不需要修改用户真实数据。

## 跨版本补充验收

`tests/fixtures/intent-reconstruction-v1-projection.json` 是当前 1.1.0 完整提示词的最小 StylePackRecord 序列化样本（pack + icon），去掉设备字段 iconPath 和 active、kind 投影 imported。对应测试逐字段对比当前 Rust 内置包的实际序列化，验证来源/版本/听写指令/选区指令/语音编辑指令/基础模式/示例/推荐模型任一行为配置变更都保持 imported，且 Store 后续初始化不覆盖。
