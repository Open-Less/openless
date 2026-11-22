# Ask / PersonalAssistant 免费与混合搜索兼容验收

日期：2026-11-23。独立分支 `appergb/ask-search-compat`，起点为 `local/ask-anything@d266ea890f54b290a413c559dfb85de12e6c0d02`。本分支只供协调者专项验收，未合入 Beta、未改版本、未启动或替换用户应用、未访问用户真实配置。

## 源到目标映射

所有应用路径相对 `openless-all/app/`。

| 来源 | 目标与适配 |
| --- | --- |
| `2870ab5849bd7e9bbc7608e72774c5d0f7292c51` 的配置切换恢复 | `src/features/settings/sections/webSearchConfig.ts`、对应纯函数测试、`WebSearchSection.tsx` 和 `e2e/web-search-settings.spec.ts`；保留本基线原设置组件、原服务 ID、原 `web_search.config` account。Hybrid 缺 key 可保存并用免费来源 |
| 同提交的聚合降级与去重 | `crates/openless-core/src/web_search.rs`、`web_search/public_sources.rs`；按语义移植，保留本基线 `RequestPolicy::Ask`、匿名不跟随重定向客户端、`on_request` 请求前取消回调、每源超时与冷却 |
| 本 Ask 基线额外兼容 | `ask_search.rs` 移除第二层 Hybrid 缺 key 提前返回；Tavily/Brave 缺 key 和 SearXNG 缺地址校验保留 |
| 同提交的 `SearchResult.providers` | 经 `SearchSource` → `SearchEvidence.source` / `AskSource` → 宿主 wire → `ask-window/client.ts` 保留；反序列化缺字段默认空，空数组不输出，直接读取网页不编造搜索提供方 |
| `c216c4790b6134bb8488e20fbc12c93eaa98a016` 的 legacy QA 验收思路 | 复用本基线 `qa_service/standalone/tests.rs` 的模型夹具与现有正式 API，新增共享工具循环和宿主工厂 HTTP 验收；没有搬入 legacy `answer_qa_with_context` 测试冒充新助手验收 |
| `740f59e4ecac8d9e43dc641d1422675acfd3be53` 的调查与交接 | 本文落实其最小适配范围；不搬入整个 Ask 或恢复 CLI/MCP Agent 平台 |

## 生产路径与证据等级

`src-tauri/src/lib.rs` 实际注册 `ask_window::chat::ask_submit` 并装配 `AskHost` / `Desktop`；`ask_window/chat.rs::AskHost::new` 使用 `backend.create_standalone_ask`，提交走原会话授权。`crates/openless-core/src/api.rs::create_standalone_ask` 将同一个 `Arc<WebSearchTool>` 交给 `StandaloneQaService` 和 `PersonalAssistant`，两者继续使用同一模型工厂。

本次宿主工厂测试实际调用该 `OpenLessBackend::create_standalone_ask`，使用独立临时数据目录、进程内凭据库、真实 `ConfiguredAskModels` 和本地 HTTP SSE 模型夹具，依次执行意图分类 → Standalone → PersonalAssistant → `web_search` 工具 → 所选搜索 → HTTP 最终模型请求 → 来源快照。测试结束清理自己的目录。没有读取用户凭据，也没有在运行中的应用启用 Ask。

`SnippetOnly` 仍表示只有搜索摘要、没有可用正文证据。模型工具结果只报告摘要未核实；只有页面状态为 `Read`、正文非空且标题相关，才允许进入 `Done.evidence`。来源事件和快照可包含未读取的摘要，但不含正文。搜索返回 URL 来自来源响应，聚合仅忽略 URL fragment 去重，不生成或重写链接。提供方信息随同一网页合并后再限制结果数量为 5。

## 验证

以下命令在应用目录执行；普通测试不会调用付费 DeepSeek：

```sh
cargo test --locked -p openless-core --features ask-attachments --lib web_search
cargo test --locked -p openless-core --features ask-attachments --lib ask_search
cargo test --locked -p openless-core --features ask-attachments --lib selected_search_chain
cargo test --locked -p openless-core --features ask-attachments --lib host_factory_selected_search_reaches_http_model_and_source_events
cargo test --locked --manifest-path src-tauri/backend-tests/Cargo.toml --features openless-core/ask-attachments --test ask_native
npx tsx src/features/settings/sections/webSearchConfig.test.ts
npx tsc --noEmit
npx vite build
OPENLESS_E2E_PREVIEW_PORT=5533 OPENLESS_E2E_FIXTURE_PORT=5534 npx playwright test e2e/web-search-settings.spec.ts --project=chromium
```

已通过：公共搜索 10 项（另 3 项默认 ignored）；Ask 搜索 4 项；Standalone 共用真实搜索工具的 Off 和 SearXNG 摘要/全空夹具 2 项；宿主原生上下文与 wire 13 项（另 1 项需用户浏览器与权限的测试 ignored）；配置纯函数；TypeScript；Vite 构建；Chromium 设置 1 项（中英文、850px、配置切换保存重开）。Vite 仍有既有大 chunk 提示。

公共搜索夹具覆盖部分源失败、CAPTCHA、超时、全空、全失败、非法链接、提供方去重合并与结果上限；Ask/助手取消测试覆盖请求丢弃、关闭 pending HTTP、取消后无完成/迟到事件、保留会话与工具身份隔离。未修改普通听写、Shift、地址栏、其他搜索服务执行方式或同步协议。

另显式通过 `selected_search_chain_free_live` 与 `selected_search_chain_hybrid_missing_key_live`：各有 5 条真实免费来源进入 Standalone 来源快照；无 DeepSeek 提供方。这两项模型是内存夹具，回答不是云模型答案。

宿主正式工厂的两项 live 也已显式执行通过（`host_factory_free_live`、`host_factory_hybrid_missing_key_live`），并与 SearXNG 工厂夹具串行执行合计 3 项通过：两次均返回 5 条 FreeSerp/Keenable 来源；`https://doc.rust-lang.org/stable/` 的 `readStatus=read`，正文进入最终 HTTP 模型请求；`https://doc.rust-lang.org/` 及 DEV Community 当次正文读取失败，另外两条未尝试读取，均未冒充正文证据。此处真实的是搜索与页面读取，最终答案仍为 HTTP 模型夹具。可复验：

```sh
cargo test --locked -p openless-core --features ask-attachments --lib host_factory -- --include-ignored --nocapture --test-threads=1
```

对 `assistant::tests` 的扩展检查结果为 9 通过、1 失败：`repeated_tool_reads_compact_then_resume_the_loop_and_finish` 在 `requests[4].instructions.contains("INTERNAL CONTINUITY COMPACTION TASK")` 断言失败。该测试只调用 voiceHistory、未走搜索，测试主体和预算/压缩实现未改；本任务未调整这条无关压缩策略，也没有将此检查称为通过。已用 `git show d266ea89` 核对该起点的 `assistant/prompt.rs`：实际提示词已经是 `INTERNAL COMPACTION ONLY`，旧断言要求的文字不存在，当前文件与起点一致。协调者应在主线验收时记录和处理这个旧契约不一致；未修改原分支，也未在原分支运行测试。

## 明确限制

- DeepSeek 官方现有 Secret 已由协调者报告 401，本任务未继续任何付费 live，也未借用第三方 key。混合成功仅有源失败夹具；真实混合含 DeepSeek 来源的成功仍等待用户更新官方 key 后受控验收。
- 模型夹具只证明配置、真实网络搜索、工具循环、证据传递与宿主 API 行为；不证明云模型的真实答案质量。
- 宿主注册表与装配路径已核对，宿主工厂和原生 wire 已执行；没有启动安装版 GUI 或通过 Tauri WebView 实际点击 `ask_submit`，故不能宣称安装版端到端验收完成。
- `web_search.config` 沿用本机 Application secret，不进入 E2EE；`providerConfigs` 只保存在原 account 内。旧 UI 保存会剥除此扩展，不能承诺旧客户端编辑后恢复各提供方配置；没有扩大 secret 同步范围。
