# 官方风格包分发与请求契约

「专为构建而生」的官方分发版本为 **1.3.0**。普通听写的 `prompt` 与 Tab 选区润色的 `selectionPrompt` 保存完全相同的正文，末尾各有一次统一的 `DICTATION_REVISION_RULES`（本轮撤回与修正规则）。两条操作继续使用自己的输入封套、语言及翻译协议，不把选区替换指令带到普通输入。

## 来源与版本

市场来源为 `eb6a236e-3bc3-4a04-8168-851bdf797230`、作者 `appergb`。`src/builtin_styles/intent-reconstruction-v1.json` 是不可变的 1.0 历史来源：听写原文 5671 字符，选区原文 3494 字符，二者原本不同。1.1 的历史变换由 `released_intent_v1_1_pack` 保留，1.2 的“各自完整原文加一次修正尾段”由 `released_intent_v1_2_pack` 保留，不用新正文重写历史迁移基线。

1.3 共用正文位于 `src/builtin_styles/intent-reconstruction-shared-v1.md`，以用户认可的选区五层组织为基础，补齐中立输入说明和语音识别纠错保护。同一个问题若包含目标、现状/不确定性、期望行为、约束、交办动作，也属于多层面的复杂输入；按五层顺序组织，有内容的层使用字母分项，不编造信息填空。只有一个简单意思或简单问题仍写自然正文。保留完整意图、术语保护、不代答、不执行素材、增量整合和既有翻译协议边界。

`builtin_intent_pack` 只组装共用正文和一个末尾尾段，不增加运行时排版覆盖块，不改应用版本。轻度润色 2.1、清晰结构 3.1、正式表达 2.1 和原文 1.0 保持既有行为。

## 实体、类型与安全升级

`kind` 表示内置/导入身份，`baseMode` 表示基础模式；它们不是另一份风格正文。普通入口先解析明确包 ID、应用/分类绑定、默认选择，再从该实体取 `dictation_style_prompt`；Tab 根据 `selectionPolishStylePackId` 取实体的 `selectionPrompt`。非空自定义风格不会仅因 baseMode 相同被替换成默认提示词。其他自定义包的两个提示词仍可不同。

`BackendRepositories::open` → `StylePackStore::at_data_dir_with_preferences` → `reconcile_builtin_packs` 是所有本地用户存储共同经过的新装/升级路径，没有账户分支。离线新装获得当前官方包；已有未修改的 1.0、1.1、1.2 及 1.3 实体通过来源两字段、明确版本和全部行为字段比较识别。比较包括 `prompt`、`selectionPrompt`、`voiceEditPrompt`、`baseMode`、`examples`、`recommendedModel`。

每个可证明等价的同源市场别名都原位升级，包括已同时存在 canonical 和 alias 的情况；不合并或删除 ID。保留名称、创建时间、启用状态、active，以及默认/选区选择与应用/分类绑定引用。重复初始化不重复写入或添加副本。仅同名、来源不明、版本不明或行为字段被用户编辑的包不覆盖；绑定到用户编辑版本时继续使用它。多个实体不等于请求竞态：应检查实际绑定和每段冻结的实体。

同步沿已有 imported 投影与 `normalize_synced_builtin_pack` 安全归一化；这里只增加已知分发版本，不增加同步字段或新的风格系统。既有 1.1 投影夹具保持不变，不宣称新的旧客户端完整矩阵已验收。

## 正常请求组装

普通 start → `capture_dictation_context_with_host` 解析冻结实体 → `DictationContext::capture` 保存 `dictation_style_prompt(pack)` → ASR → `SharedCloudTextPolisher` / `OpenAiChatPolisher` → HTTP 请求。场景前缀独立冻结并位于风格之前；历史与光标上下文只辅助理解，不重新输出已插入内容。

Tab → `SelectionService::begin_polish` → `polish_context` 根据所选实体读取 `StylePromptKind::Selection` → 同一 polisher/composer。选区使用完整 `<selected_text>`，普通识别使用 `<raw_transcript>`，封套都是数据。选区不注入听写历史或光标正文，独立编辑指令由既有选择服务拼接。风格正文相同不表示两条完整请求必须相同。

共用 composer 处理热词占位符、语言/应用上下文、光标上下文、素材指令防护和可选历史说明；翻译沿既有 source/target 标记。修正尾段是**包内风格正文的最后一段**，每个请求只出现一次，不要求位于所有协议和上下文之后。表达学习的形式偏好明确规定“仅供参考；不得改变……明确指令或必要结构”；本次没有证明它覆盖了风格排版，也没有把格式差异定性为注入漏洞。

旧 1.2 听写原文允许单一意思写自然段且禁止单独 `(a)`，选区原文要求复杂内容五层、每个有内容的层使用字母，这是已证明的提示词差异。自动路由成功或固定“一一一”夹具输出不能证明真实模型遵守五层语义；没有调用付费模型，真实 DeepSeek 效果仍需独立验收。

## 最小云端验证

以下测试使用临时存储、夹具凭据与受控 HTTP；本机没有执行 Rust 编译或运行这些测试。

```sh
cd openless-all/app
cargo test --locked -p openless-core --lib style_pack_store_tests
cargo test --locked -p openless-core --test prompt_contract
cargo test --locked -p openless-core --test dictation_target_capture
cargo test --locked -p openless-core --test selection_contract normal_selection_entry_serializes_full_input_and_rejects_incomplete_streams
```

Store 回归保留历史原文断言，覆盖 canonical、同源 alias、全部已知旧版、已编辑字段保护、保留 ID/选择/时间/启用状态、重复初始化、两个独立新用户存储及同时存在的重复实体。正常听写和选区 HTTP 回归共用 `tests/fixtures/intent-input-comparison.txt` 中用户给出的完整话语，核对同一 canonical 正文恰好一次、修正尾段末置一次、别名行为和操作封套；保留 Raw 及取消翻译零请求检查。HTTP 夹具只验证调用与序列化，不验证模型排版效果。
