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

后继场景工作应单独冻结场景前缀，在最终请求合成时形成：场景前缀 → 风格 → 仅非内置口述修正尾缀 → 本次完整输入。不得将场景前缀写回用于 Raw 身份判断的 canonical style_system_prompt。本次只改 `dictation_context.rs` 初始化的一行，不实现场景采集。

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

## 可选真实模型验收入口

复用搜索组的 cargo ignored 测试约定，在应用目录运行：

```sh
cargo test --locked -p openless-core --lib style_live_official_deepseek -- --ignored --nocapture --test-threads=1
```

普通测试会忽略此入口。`.github/workflows/ci.yml` 的手动参数 `live_acceptance` 默认 off；只有显式选择 style/search 才运行官方模型验收并跳过普通多平台检查。先无凭据构建，只有执行步骤从仓库 Secret 注入 DEEPSEEK_API_KEY，凭据不出现在文件、argv 或日志。搜索选项需要对应搜索提交已在所选基线上，测试不存在会失败而非空跑成功。不要为获取 Secret 合并到 beta。

风格测试固定官方 endpoint `https://api.deepseek.com/v1` 和源码配置模型 `deepseek-flash`，不接收任意 endpoint 参数，也不等于核实用户 Ark 渠道型号。复用 SharedCloudTextPolisher、冻结的 DictationContext 和实际提示词合成。八个主请求分别覆盖范围扩大、改口、并列选项、明确撤回、简单日常、问句、未决冲突、长输入；意图包与清晰结构为主，日常例走内置翻译，问句例走非内置翻译。翻译协议缺失触发的已有 fallback 也计入总上限十次；仅在测试构建中启用发送计数并禁用网络重试，接口或凭据错误立即停止。

每例以 LIVE_STYLE JSON 记录公开响应 model、请求模型、渠道、style ID、输入、整理源文（翻译时）和最终输出，附预期语义和 pending review 标记；不记录 SSE 原始数据或隐藏推理。HTTP 成功不代表语义通过，必须阅读输出逐项确认，实际结果单独记录。GitHub 保存公共输出日志 artifact，不包含凭据。常规检查在这类手动运行中被跳过，不能说它们已经通过。

### 本次真实执行结果（2026-11-23）

仅显式手动触发一次：[运行 45056385541](https://github.com/Open-Less/openless/actions/runs/45056385541)，提交 f10c62b6。GitHub 无密钥编译成功，普通平台检查全部跳过；执行步骤获得 Secret，但第一例 scope 返回 Provider 错误后停止，未取得正文或响应模型，其余七例未运行。没有再次触发、重试或扩展付费调用。

[逐例结果证据](evidence/style-live-45056385541.json)记录官方 endpoint、请求模型 deepseek-flash、首例输入和预期，以及所有未执行例。该结果不能证明任何真实语义遵循，也不能核实用户 Ark 下的型号；轻度和正式没有真实模型覆盖。旧入口没有记录 HTTP 状态码，无法据 Provider 分类区分凭据、模型和服务问题。后续仅补充测试入口的安全诊断：打印发送序号/HTTP状态码及失败例元数据，不打印错误响应正文、凭据或隐藏推理；已无密钥编译，未再次运行真实请求。后续排查和重跑须另行协调，不把失败隐藏为语义通过。

## 官方诊断前核对与同步组交叉证据

2026-11-23 读取 [DeepSeek 官方入门](https://api-docs.deepseek.com/)及[模型列表说明](https://api-docs.deepseek.com/api/list-models/)，确认 deepseek-flash 仍是公开模型名；入门说明由 DeepSeek-V4.1-Flash-1017 提供服务，这不等于本次响应已经报告该型号。[官方 WorkBuddy/CodeBuddy 接入](https://api-docs.deepseek.com/quick_start/agent_integrations/workbuddy/)包含 `https://api.deepseek.com/v1/chat/completions`，因此未随机更换型号或路径。[官方错误说明](https://api-docs.deepseek.com/quick_start/error_codes/)区分 401 鉴权、402 余额、422 参数等，需要实际状态码才能定位外部条件。

在 29efa3cb 的诊断基础上，4d450e6e 增加手动 style-diagnostic 选项：仅首例、最多一次实际 HTTP 发送、无重试，不继续其余七例；只保留状态码、最终 URL、白名单公开 error.code 和公开响应 model，不保存错误正文、请求头或凭据。已通过无密钥编译和 workflow YAML 语法检查，未重跑普通平台或已过夹具。

同步组已交付 38a8ed0a（依赖风格 helper 3330347d），本会话只读了其 `docs/development/sync-v1-compatibility.md`：真实已发布 v2.0.2 stable 与 v2.0.3 Beta.1 各覆盖 canonical/alias × 未编辑/编辑三类提示词四组，经原生产提交服务 router/Store、SQLite 重开和新旧客户端原生仓储往返，选择、实体和编辑均保留，编辑包保持 imported。此为同步组实现与证据，不等于本会话重复运行；没有安装旧 GUI 或真实模型效果证据，也不承诺更早 v2.0.0 的既有 registry 差异兼容。风格工作区没有合入或修改同步模块。

### 一次有界诊断结论

[诊断运行 45057806632](https://github.com/Open-Less/openless/actions/runs/45057806632)使用 4d450e6e，只重发 scope 首例一次：最终 URL 为 `https://api.deepseek.com/v1/chat/completions`，HTTP 401，公开 error.code 为 invalid_request_error，response model 与正文均为空。公开错误码的名字不能当成 HTTP 400；依据官方错误说明，实际 HTTP 401 对应鉴权失败，不是模型效果证据。

[安全诊断记录](evidence/style-diagnostic-45057806632.json)保存实际发送序号 1、状态码、最终 URL、公开错误码及首例元数据。没有重试、没有额外 models GET、没有补跑其余七例，也没有替换凭据或修改用户 Ark 设置。GitHub 无密钥构建成功，普通平台 CI 跳过；本轮仅做本机无密钥编译与 YAML 语法检查，没有重跑已过夹具。

外部处理事项已明确：由仓库凭据维护者在官方 DeepSeek 账号中核对有效 API Key，并按需更新仓库 DEEPSEEK_API_KEY Secret；不要回传或导出密钥。此结论不证明余额状态，也未核实用户 Ark 下的模型。凭据处理后才具备继续真实语义验收的条件，首例与其余七例均仍没有语义成功结论。
