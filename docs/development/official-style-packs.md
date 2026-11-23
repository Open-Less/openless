# 官方风格包、整体意图与口述修正

风格恢复基于 `46f55c087985ceea5f94ba8917a29cf5dbeadb4d`，只修改 Core 风格分发、安全升级和定向验证，不修改应用版本、UI、安装中的应用或真实用户数据。

## 来源与实际路径

「专为构建而生」的正文来源是市场包 `eb6a236e-3bc3-4a04-8168-851bdf797230`，来源作者 `appergb`，市场版本 `1.0.0`，云端更新时间 `2026-11-22T08:52:19.476456308+00:00`。已核实实际 [下载 ZIP](https://apic.openless.top/packs/eb6a236e-3bc3-4a04-8168-851bdf797230/download) 中的 `manifest.json`、`prompt.md`、`examples.json`：听写正文 5671 字符、选区正文 3494 字符，voiceEditPrompt 缺省为空串，examples 按 examplesFile 读取。详情页的 prompt 展示拼接了听写和选区指令，不能当作听写正文。

Core 的 `src/builtin_styles/intent-reconstruction-v1.json` 是用户当时本机更新后已安装的不可变快照，与该实际 ZIP 的正文及所有行为配置逐字段一致。factory 仅将 kind 设为 builtin、分发版本设为 `1.2.0`；prompt、selectionPrompt、voiceEditPrompt、examples、recommendedModel 等有效配置保持原样。`1.2.0` 是修复分发版本，不代表市场正文升级：正文仍来自上述已核实的市场 `1.0.0` payload。五层、空层、字母分项、术语、改口和未决规则不作任何改写，也不追加通用口述修正尾段。

正常听写入口：`OpenLessBackend::capture_dictation_context` 解析所选包，`DictationContext::capture` 冻结 `dictation_style_prompt(pack)`。传统 ASR 后润色经 `cloud_providers.rs` 的润色分发调用 OpenAI/Gemini 共用的 `prompt_compose`；组合翻译使用现有源文和译文标记协议。多模态经同文件 `build_omni_prompt`，从冻结的 context 构建相同系统提示词，把完整音频送入 Omni provider；多模态翻译仍沿用现有仅输出译文规则，不增加第二套组合路径。选区编辑继续读取 selectionPrompt；voiceEditPrompt 为空时沿用原有默认。

## 改了哪些包

| 包 | 内置版本 | 原因 |
| --- | --- | --- |
| 专为构建而生 | 1.0.0 / 1.1.0 → 1.2.0 | 逐字恢复用户指定的实际市场包正文；1.1.0 曾替换五层及空层规则并追加尾段，现只作为可信迁移基线保留。 |
| 清晰结构 | 3.0.0 → 3.1.0 | 加强只整理不回答、完整阅读后组织、扩大范围更新整体目标、保留细节和未决问题；加入口述修正规则。 |
| 轻度润色 | 2.0.0 → 2.1.0 | 加强任务边界、完整信息和语气保护，明确原意优先于旧字数比例；加入口述修正规则。 |
| 正式表达 | 2.0.0 → 2.1.0 | 同上，尤其保护不确定程度和请求目的，避免正式化变成代答或补充方案。 |
| 原文 | 保持 1.0.0 | 保持原文直通及取消翻译后不请求模型。 |

其他官方包的现有规则保持不变。HTTP 夹具只证明生产请求序列化，不调用真实模型，也不宣称模型语义 A/B 通过。

## 迁移、身份与兼容

`style_pack_store::reconcile_builtin_packs` 复用现有内置版本更新。旧官方三包只有提示词全文等于随源码保留的已知发行文本才自动升级；用户修改的提示词及无法证明来源的历史文本不覆盖。普通官方包只更新 dictation prompt 和版本，保留用户的选区、语音编辑、名称、启用状态和其他配置。

专为构建而生采用原本地 ID 和市场来源。`released_intent_v1_1_pack` 保留原 1.1 工厂变换，既有 `tests/fixtures/intent-reconstruction-v1-projection.json` 固定其已发行内容，不能随修复版本更新。安全识别同时要求来源两字段、明确已知的 1.0.0 / 1.1.0 / 1.2.0 版本以及全部行为字段等价。对 ID 不同的已安装同源包，必须同时匹配 originPackId 与 originAuthorLogin，并逐项比较 prompt、selectionPrompt、voiceEditPrompt、baseMode、examples、recommendedModel 后，才认定等价并原位接管升级；保留原 ID、名称、启用状态、当前选择和创建时间，不改分类/应用绑定和历史引用。名称相同不足以接管。来源相同但内容修改的不同 ID 包保留为独立用户版本，官方包另行提供；占用官方 ID 但内容修改的包也不覆盖。不会删除或重映射既有引用。

只给四个历史模式迁移 preferences 的旧提示词；按明确 builtin ID 同步各模式偏好，避免新增 structured 模式的包抢占旧清晰结构。离线新装增加第五包，旧用户原选择不变；重复启动不重复写入；旧快照经现有 normalize 路径重新补齐并保留别名 ID。

没有新字段、枚举或同步文档类型，但仅能解码 JSON 并不足以证明跨版本可恢复：同步组已用真实 v2.0.0/v2.0.2 确认，native preflight 会拒绝旧四个内置 ID 之外的 builtin，导致整次恢复失败。同步组负责在 v1 导出将新 builtin 投影为 imported，保留完整实体和引用；新版 native preflight 在身份检查前调用 style_packs::normalize_synced_builtin_pack，只在来源、版本和行为配置等价时恢复 builtin，旧客户端改过的内容保持 imported。此纯函数不改 ID、提示词或选择，来源别名也保留原 ID。同步组还必须允许新官方 ID 被用户编辑后继续作为 imported，不能仅凭 ID 强制内置身份。恢复沿用已接入的 helper 与原同步协议，不修改同步模块或重跑旧客户端完整矩阵。

## 场景合成契约

`style_packs::dictation_style_prompt(&StylePack)` 是唯一风格及非内置尾缀入口。内置身份要求 kind、稳定 ID 或来源身份及随发行提供的提示词一致，不能只看包名；已修改的内置包按自定义请求处理。内置修正规则保存在包内，非内置在请求时追加一次，绝不写回用户内容。若用户提示词已含完整相同规则，则不重复追加。规则只确定素材里的有效意思，不覆盖固定输出、风格格式或翻译协议。

场景实现现已单独冻结场景前缀（见[冻结场景契约](app-style-routing.md#冻结应用场景前缀)），在最终请求合成时形成：场景前缀 → 风格 → 仅非内置口述修正尾缀 → 本次完整输入。不得将场景前缀写回用于 Raw 身份判断的 canonical style_system_prompt。风格组件负责 `dictation_context.rs` 的风格初始化；场景组件负责独立前缀采集及请求时前置，不写回 canonical 风格文本。

## 恢复的定向验证

源码路径相对 `openless-all/app/crates/openless-core/`。以下测试仅使用临时 Store、夹具凭据及本机 HTTP 服务，不读取或写入用户实际配置。

- `src/style_pack_store_tests.rs`：factory 与不可变源完整相等、新装可用；已发行 1.1 投影夹具保持不变；1.0 / 1.1 / 1.2 的 canonical 和同源 alias 归一化及原位升级，保留 ID、名称、禁用、active、创建时间、听写和选区选择；重开不重复写入；任一行为字段、来源或版本被编辑则不覆盖。升级不重映射 ID，因此绑定和历史引用继续指向原实体。
- `tests/prompt_contract.rs`：恢复包完整正文进入普通润色和翻译系统提示词，无 1.1 覆盖块、无额外修正尾段；其他风格既有尾段保持一次。
- `tests/dictation_target_capture.rs::routed_dictation_keeps_the_frozen_pack_and_delivers_to_current_focus`：正常 start → ASR → HTTP 请求中保留完整目标风格及翻译协议，沿用场景冻结；Raw 及取消翻译维持零请求。
- `tests/selection_contract.rs::normal_selection_entry_serializes_full_input_and_rejects_incomplete_streams`：选区生产 HTTP 请求完整保留恢复后的 selectionPrompt，无旧覆盖块或额外修正尾段，仍验证全文输入与不完整响应保护。
- `cloud_sync_e2ee_store::tests::v1_compat::projected_builtin_and_edited_alias_restore_without_losing_selection`：复用现有原生恢复入口，验证已发行 1.1 投影与编辑版本的选择保留；不扩展旧客户端兼容承诺。

请求保留现有 `{{HOTWORDS}}` 热词占位符替换及尾端空白处理；逐字正文比较在 factory 和不可变源之间进行，请求比较使用同一既有热词合成入口，不改写包内规则。

这些验证不等于真实模型效果、GUI 或安装包验收；后续打包应使用包含该修复的提交。
