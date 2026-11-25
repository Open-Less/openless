# 免费聚合搜索与 DeepSeek 混合搜索

核实日期：2026-11-23。开发基线：local/beta `51b65224`。没有修改版本号、启用 Ask Anything、发布应用或建立线上搜索代理。

## 原有能力与本次修改

基线已有 `auto/free/free_serp/hybrid/bing/tavily/brave/searxng/off`，免费模式并发调用 FreeSerp 和 Keenable，混合模式额外调用 DeepSeek 官方独立搜索 API。已有每源 8 秒、整次 15 秒超时、每源最多 5 条、按名次交错合并、去掉 URL 片段后的重复判断、429 冷却和最多前三页的正文摘录；本次复用这些机制，没有重新实现搜索系统。

本次修复服务切换清空配置、混合模式缺 key 时提前阻断免费结果、所有来源为空仍返回成功三处问题。聚合结果新增 `providers` 数组，保留提供方和网页链接；同一网页被多个来源命中时，合并提供方后再限制总数为 5 条。保留第一个来源的 URL、标题和摘要，不用模型生成来源。

设置沿用「设置 → 高级功能 → Web Search → 搜索服务」及原选择弹窗，显示「免费聚合搜索」和「免费聚合搜索 + DeepSeek Web Search」。仍保留其他服务、原持久化 ID、独立 key 和 SearXNG 地址。

## 来源、费用与官方证据

| 来源 | 当前接入及条件 | 免费额度与限制 | 官方资料 |
| --- | --- | --- | --- |
| FreeSerp | 已有 GET `https://freeserp.ai/api.php?index=web&q=…&size=5`；无需账号、key 或信用卡 | 官方未设固定配额，但要求合理请求速率、缓存和大流量时联系；不构成永久无限可用承诺。网页索引不可用而降到站点索引时，现有代码拒绝把它当正常网页搜索 | [官网及 FAQ](https://freeserp.ai/)、[接口文档](https://freeserp.ai/docs.php) |
| Keenable | 已有 POST `https://api.keenable.ai/v1/search/public`，JSON 查询并携带 `X-Keenable-Title: OpenLess`；此公共路径无需账号、key 或信用卡 | 每 IP 每端点 1,000 次/小时、10 次/秒，与同出口其他用户共享。公共查询按默认数据策略会记录；不发送隐私文本。可选注册 key 每月 100,000 次免费、无需卡，超额预付费 $4/千次；本实现继续使用公共路径，没有添加 keyed 计费模式 | [无 key 接入](https://docs.keenable.ai/guides/keyless-access.md)、[限额](https://docs.keenable.ai/rate-limits.md)、[余额](https://docs.keenable.ai/credits.md)、[首页](https://keenable.ai/) |
| DeepSeek | 已有 POST `https://api.deepseek.com/v1/tools/search`，用户自己的官方 API key 通过 Bearer 认证；只发送查询、`count:5`、`need_content:false`，查询最多 128 个 Unicode 字符 | **不是免费承诺**。独立 Search API 每千次有非空结果的成功请求 $0.75，失败、空结果、超时不收费；账号搜索限 100 QPS。须有可用官方账号/key 和余额，不能使用第三方模型网关 key。没有证据保证账户赠额或免信用卡开户，因此不作承诺 | [Search API](https://api-docs.deepseek.com/api/search/)、[Search & Fetch](https://api-docs.deepseek.com/guides/search/)、[当前价格](https://api-docs.deepseek.com/quick_start/pricing/) |

这里的 DeepSeek 是官方自有搜索引擎返回结构化来源，**不是让普通模型猜链接**。混合搜索将检索结果交给用户原有问答模型；模型调用的 token 费用由该模型服务另计，没有另造计费系统。

另一个原有功能「DeepSeek 大统一」通过 `deepseek_suite.config` 中的独立 key 调用 `deepseek-flash`，在 Chat Completions 请求中声明 `deepseek_web_search` 和 `deepseek_read_url`，由服务端模型自行决定搜索。这与混合模式的独立 Search API 不同。根据 [Web Tools](https://api-docs.deepseek.com/guides/web_tools/) 和当前价格，自 2026-11-18 12:00 北京时间起，服务端搜索/读取工具不另收费，但返回内容加入模型上下文，计模型 token 费用；不能把它解释为整个 DeepSeek 请求免费。本次未改变大统一路由或凭据。

## 调用链与边界

下列路径相对 `openless-all/app/`：

| 要求 | 实际入口与源码位置 | 验证和限制 |
| --- | --- | --- |
| 复用设置和服务选择 | `src/features/settings/shell/tabs.tsx::AdvancedTab` → `sections/WebSearchSection.tsx` → `webSearchConfig.ts` | 旧枚举保留；`auto` 显示免费聚合但未切换时保留原 ID。前端回归覆盖逐服务切换、保存重开、key 与地址恢复 |
| 凭据隔离、配置恢复 | 原 Application account `web_search.config` → `src-tauri/src/commands/credentials.rs` → `persistence/credentials.rs::set_web_search_config` | 新增可选 JSON `providerConfigs: { [原服务ID]: {apiKey, endpoint} }`；顶层三字段继续代表当前服务。宿主验证已知字段后保存原字符串；不迁移到 preferences，不新增 secret account |
| Ask 实际调用 | `src-tauri/src/commands/qa.rs` → `crates/openless-core/src/qa_service.rs` → `cloud_providers.rs` 的问答处理 → `qa_search_context` → `web_search::search` | 普通/多模态问答先规划公开搜索词，再插入检索上下文；大统一沿原服务端工具路径。当前 `dev.noask` 同时被前端 `src/lib/appVersion.ts` 和宿主 `recording_companion.rs::ask_anything_available` 禁用，**没有恢复**，因此不声称安装版 Ask 点击到模型的端到端验收已通过 |
| Agent consumer 核查 | `crates/openless-core/src/less_computer.rs::run_once` 和 `coding_agent.rs::CodingAgentService::run_test` 是已保留的 Core runner 消费者；宿主进程适配当前未接入 | 本搜索设置未作用于这些 runner。Claude 白名单中的 `WebSearch` 与 Codex `web_search` 事件解析只能说明 CLI 协议封装存在，不能证明生产 Agent 可用或所选搜索已接入；详见下文 |
| 并发、部分失败、全空 | `crates/openless-core/src/web_search/public_sources.rs::query/Source::query/merge` | 免费 fan-out 两源，混合三源；每源错误/拒绝/超时独立处理。有有效结果就返回其余结果，混合缺 key 仅跳过 DeepSeek；全空返回明确错误，全失败返回来源错误。CAPTCHA HTML、JSON error、HTTP 非成功不伪装成空成功 |
| 排序与溯源 | 同文件 `merge`、`web_search.rs::SearchResult/context` | 按来源固定顺序、名次轮转，去重仅忽略 URL fragment，不擅自删除有语义的查询参数。结果包含原始可引用 HTTP(S) 链接和 `providers`；共同命中合并提供方。不保证搜索结果本身正确或实时 |
| 取消与超时 | `cloud_providers.rs::qa_search_context`、`web_search.rs::search/search_inner` | 共享入口每 25ms 检查原取消信号，取消时丢弃搜索 future 及未完成请求；无需新会话 ID。总搜索最多 15 秒、源请求最多 8 秒、最多前三页摘录各 5 秒并发。正文读取失败保留检索摘要 |
| 独立流程保持 | 改动只在上述搜索模块及搜索设置 | 普通听写、Shift 翻译、浏览器地址栏搜索没有改动；未把它们导入搜索设置，未运行这些独立功能的硬件回归 |

### 配置兼容与同步限制

当前新版 UI 保存并重开后恢复各服务配置；旧版仍能读取顶层活动服务。**旧版搜索设置 UI 只重建三个顶层字段，旧版编辑并保存会删除 `providerConfigs`**，因此配置集合恢复要求使用包含提交 `2870ab58` 的客户端，这是最低能力基线（本任务没有改版本号），不能承诺跨旧版编辑保留。没有对已丢失的历史 key 做虚构恢复。

已与同步组核对：基线 E2EE 只导出 Channels 和 ProviderCredentials 的 ASR/LLM/Omni 白名单，Application 的 `web_search.config` 当前不在其中。这里的集合仍放原本机 secret，不把 key 混进 Portable/preferences，不承诺其跨设备同步；同步协议扩展需另行由同步负责人实现和验收。

## 验证与真实测试

实施日直接请求 FreeSerp 和 Keenable 均返回有效结果；随后用正常共享业务入口 `web_search::search` 执行真实搜索：

- 免费模式：`Rust language documentation`，返回 5 条，包含 [Rust 文档](https://doc.rust-lang.org/)（Keenable）、[Rust 标准库](https://doc.rust-lang.org/std)（Keenable）、[GitLab Rust 指南](https://about.gitlab.com/blog/rust-programming-language/)（FreeSerp）。
- 混合模式缺 key：`Rust programming official documentation`，返回 5 条免费来源，包括 [Rust 文档](https://doc.rust-lang.org/) 和 [Rust Reference](https://doc.rust-lang.org/reference)；这是**真实降级验收，不是 DeepSeek 成功验收**。

可控回归使用原解析、合并、源请求、共享取消入口，包括单源/多源失败、全空、全失败、非法链接、网页索引不可用、HTTP 200 CAPTCHA、本地 HTTP CAPTCHA 响应与有效源并存、源超时关闭请求、有有效结果时保留来源、去重后提供方合并及输出上限。前端纯函数回归覆盖旧配置导入、`auto` ID、来回切换和序列化重开；浏览器设置回归使用独立 fixture 及指定 5531/5532 端口，不修改真实用户设置。

最终检查结果：搜索 fixture 9 项通过、3 项 live 默认忽略；共享取消回归 1 项通过；前端配置回归和 TypeScript 检查通过；Chromium 设置回归 1 项通过（含中英文、850px 窗口）；Vite 产物构建通过，仅有既有大 chunk 提示。两项上述免费及降级 live 测试另行显式运行通过。

最小命令（应用目录）：

```sh
npx tsx src/features/settings/sections/webSearchConfig.test.ts
npx tsc --noEmit
cargo test --locked -p openless-core --lib web_search
cargo test --locked -p openless-core --lib qa_search_cancels_while_waiting_for_a_response
cargo test --locked -p openless-core --lib public_search_smoke -- --ignored --nocapture
cargo test --locked -p openless-core --lib hybrid_missing_key_preserves_live_free_sources -- --ignored --nocapture
OPENLESS_E2E_PREVIEW_PORT=5531 OPENLESS_E2E_FIXTURE_PORT=5532 npx playwright test e2e/web-search-settings.spec.ts --project=chromium
```

没有读取、导出或打印仓库 Secret。进程环境未提供 `DEEPSEEK_API_KEY`，本次未取得可用于验收的官方 key，DeepSeek 真实成功、余额/额度和收费扣款未验证。保留以下**显式付费且默认忽略**的最小入口，须由执行环境安全注入官方 `DEEPSEEK_API_KEY` 后主动运行，普通 PR 不运行；命令不携带 key，测试只打印来源及当前收费说明，必须实际包含 DeepSeek 提供方才通过，不能靠免费降级通过：

```sh
cargo test --locked -p openless-core --lib hybrid_live_official_deepseek -- --ignored --nocapture
```

无需用户更改正在运行的应用。协调者可安排上述受控 DeepSeek live 检查，再决定合入；当前未验证部分不得被描述成已完成的真实混合成功验收。

## Ask Core 补充验收：配置确实进入最终模型请求

`crates/openless-core/tests/qa_web_search_contract.rs` 直接调用桌面 `qa_adapter.rs:550` 和 Linux `qa.rs:361` 使用的公共函数 `answer_qa_with_context`，而非只调用搜索工具。测试在进程内创建独立 `InMemoryCredentialStore`，写入原 `web_search.config` 及本地模型地址，不读写用户配置，不启用 Ask Anything。

两项默认 fixture 回归已通过：

- 选择 SearXNG：Core 读取原 secret 中的服务，先向本地模型发送规划请求；模型夹具返回公开查询；Core 向所选本地 `/search` 发起实际 HTTP 查询；随后最终模型请求包含原始来源 URL 和摘要，并声明所选 SearXNG。两次模型请求都未开启 DeepSeek 大统一工具，输出事件的 `sessionId` 保持不变。
- 选择 Off：只有一次最终模型请求，没有规划请求、搜索 HTTP 请求或检索数据注入。通用 system prompt 仍包含引用 WebSearchData 的说明，不把该说明误判为实际搜索结果。

另有两项默认 ignored 的真实来源检查，已分别显式运行通过：免费、Hybrid 缺 key。它们同样走完整 `answer_qa_with_context`，读取各自进程内配置，由本地模型夹具规划并接收最终请求；真实来源均包含 FreeSerp 与 Keenable 的链接及 `providers`。这证明 **Ask Core 的服务选择生效、检索结果进入最终模型请求**；回答正文由本地 HTTP 夹具生成，不是实际云模型回答，且不是安装版 Ask UI 端到端验收，也不是 DeepSeek 付费成功验收。

```sh
cargo test --locked -p openless-core --test qa_web_search_contract
cargo test --locked -p openless-core --test qa_web_search_contract qa_selected_free_reaches_answer_request_live -- --ignored --nocapture
cargo test --locked -p openless-core --test qa_web_search_contract qa_selected_hybrid_without_key_reaches_answer_request_live -- --ignored --nocapture
```

## Agent 的实际缺口与需要协调者决定的范围

不能把“能解析外部 CLI 的搜索事件”等同于“Agent 已使用所选免费/混合搜索”。针对当前基线与本分支逐入口核对的结果如下，以下位置相对 `openless-all/app/`，没有声称已核对整个系统。

| 已存在的代码 | 调用与准确限制 |
| --- | --- |
| `crates/openless-core/src/api.rs:5033` 的 `submit_less_computer`、`:5073` 的 `build_less_computer_request` | 读普通 preferences 中的 CLI provider、工作目录、模型和权限，构造 `LessComputerRunRequest`；不读 `web_search.config`，没有搜索字段 |
| `crates/openless-core/src/less_computer.rs:786` 的 `run_once` | 将原 `sessionId` 和任务包进 `CodingAgentRequest`，调用 `CodingAgentRunner::run_streaming`；没有调用 `web_search::search`，也没有注入检索数据 |
| `crates/openless-core/src/coding_agent.rs:1876` 的 `CodingAgentService::run_test` | 设置检测/测试路径也构造该 request 后调用同一 runner，不读搜索凭据；不是另一套问答 Agent |
| 同文件 `build_agent_command:884`、`build_claude_args:481`、`build_opencode_args:538`、`build_codex_args:557`、`build_dsh_patch_yaml:602` | Claude 预设 `WebSearch` 许可而禁 `WebFetch`；OpenCode 注入权限配置；Codex 使用 `exec` JSON 输出；dsh patch 只有事件插件与任务。都没有把所选搜索配置注册成工具，没有 app-owned tool handler。外部 CLI 自带工具如何执行不由这些事件解析器控制 |
| 同文件 `CodingAgentProcessAdapter:443`、`CodingAgentRunner:1005`、`run_process:1130` | 进程接口只有执行命令/输出行/取消；JSON 工具事件是输出状态，不是发回 Core 等待处理的搜索 RPC。不能把解析 `mcp_tool_call` 或 `web_search` 当成完成工具接管 |
| `crates/openless-core/src/domains.rs:2143/2173/2184`、`api.rs:3179`、`less_computer.rs:116` | 默认 `coding_agent_process=None`；仅当宿主调用 `configure_coding_agent_process` 才绑定 runner。仓库搜索该配置方法及 trait 实现，**未发现生产宿主配置调用或进程适配实现，只有 Core fixture 实现**；未绑定 runner 明确返回 Unsupported。不能据保留的类型声称安装版 Agent 真在执行外部 CLI |
| `src-tauri/src/commands/qa.rs:210` 与 `src-tauri/src/lib.rs` 的命令注册 | 保留 `less_computer_submit_text` 函数，但当前注册表未发现此命令；前端文件清单也未发现 LessComputerPanel/CodingAgent 设置面板。Core 的门控仍要求 `testing_features_enabled && coding_agent_enabled`（`shared_types.rs:975`）。未恢复这些入口 |
| `coding_agent.rs:1793` 的 MCP 探测 | 只对 Claude 执行 `mcp list` 解析用户已有 MCP 健康状态；MCP 是外部模型连接工具的协议，此处没有创建 OpenLess 搜索工具或按会话注入配置。不能从这个探测推断所有 CLI 的接入能力均已实现 |

因此，当前 `51b65224` 搜索开发基线 **没有已证明会消费本设置的生产 Agent 入口**；Ask 与 Agent 共同使用所选服务这一整体验收尚不成立。本次没有修改这些运行路径或偷偷替换其默认搜索工具，也没有为补足验收而恢复 Agent 或 Ask。

上述结论仅限本次 `51b65224` 开发基线与搜索分支，不能扩大为全项目无 Agent 能力。后续已只读核对指定的独立 Ask 分支，找到了可复用的 `PersonalAssistant`；应优先按下文审查该既有入口，不应据此要求用户选择全新 Agent 产品，也不应优先新建 CLI/MCP 桥。当前 Beta 的启用和集成仍须协调者明确验收。

## 共用受控 CI

已与风格负责人 `dispatch:ctx_445fa40c603a` 共享原 ignored `hybrid_live_official_deepseek` 入口和安全约定，复用其 `f10c62b6` 的单一 `ci.yml` 手动 `live_acceptance=off/style/search` 方案，默认关闭，不另建框架。本分支没有合入或修改其 CI；协调者须在包含两组已验收依赖的基线上运行一次 `live_acceptance=search`，仅该执行步骤通过环境注入官方 Secret，不导出 key，不在普通 PR 自动付费。搜索测试一次中性查询、不重试，必须实际包含 DeepSeek 提供方才通过；该受控检查结果当前仍待回传。

## 指定 Ask 分支的只读复核：已有共享搜索的 Agent

按协调者要求，另用 `git show`、`git grep`、`git log` 和 Git 差异只读核对以下引用，没有切换或修改这些分支，没有合入未验收 Ask 代码，没有启动其宿主或开启 Beta 灰色功能。

| 分支与检查时的 commit | 发现 | 集成含义 |
| --- | --- | --- |
| `local/ask-anything` — `d266ea890f54b290a413c559dfb85de12e6c0d02` | 已集成独立 Ask、`PersonalAssistant` 及 Compact；有真实宿主注册和共享搜索工具装配 | 这是本地 Beta 上的 Ask 专项候选基线，不是当前 `51b65224` 的后继；与本搜索分支共同祖先为 `33f15149`，不可直接覆盖或当作无冲突快进 |
| `deferred/ask-anything-20261122` — `ba3fc60462d20c749c42b2669f0198795b279fba` | 同样有 Agent 工具循环、共享搜索和宿主接入；其历史还含 `661685ee` 冻结及 `ba3fc604` Compact 调整 | 协调者可将其作为专项验收依据；不能因找到源码就解冻、发布或恢复入口 |
| `review/main-ask-beta-20261121` — `955870409438c6f792c0a951b60af0e9f142a3f1` | 同样有共享搜索的 `PersonalAssistant`，包含 `07353f9c5d542915ef8cec137f105e9d90ca878b` 的助手与原生上下文集成 | 比 deferred 版本少后续 Compact 调整；这里的已有 Agent 是只读个人助手，不是旧 Less Computer 的外部 CLI |

三处分支都没有发现生产 `CodingAgentProcessAdapter` 实现/配置调用，旧 CLI 路径的限制仍成立；但这**不能排除另一条已经存在的 PersonalAssistant 路径**。以下位置在三处分支的关键装配处一致，路径相对 `openless-all/app/`：

1. `src-tauri/src/lib.rs:91` 声明桌面 `ask_window`，`:896–897` 实际装配 `AskHost` 和 `Desktop`，`:371–378` 注册 `ask_window_open`、`ask_submit`、快照与取消等宿主命令。`src-tauri/src/ask_window/chat.rs:46–84` 的 `AskHost::new` 调用 `backend.create_standalone_ask`，`:694` 的 `ask_submit` 处理真实提交及会话授权。因此不只是孤立 Core 类型。
2. `crates/openless-core/src/api.rs:4381–4420` 的 `create_standalone_ask` 从原 credential store 创建 **同一个** `Arc<WebSearchTool>`，同时交给 `PersonalAssistant::new(AssistantDependencies { search: search.clone(), … })` 和 `StandaloneQaService`。同一个已配置模型工厂、usage recorder 也继续复用，无需建立第二套 Agent 或搜索设置。
3. `qa_service/standalone.rs:1757–1772` 的普通问答分支构造 `AssistantRequest` 并调用 `deps.assistant.run`。`assistant.rs:59–135` 是有限、可取消的模型 → 工具 → 结果 → 模型循环，支持多次工具调用；不是预检索一次后让外部 CLI 自行搜索。
4. `assistant/tools.rs:163–210` 处理 `AskToolCall::WebSearch { public_query }`：调用 `deps.search.enabled()`、`deps.search.search(AskSearchRequest { key, public_query, … })`，保留 `conversation_id/turn_id/tool_call_id`，转发真实来源事件并把工具结果送回循环。这里的现成工具接口已经满足“指定 Agent 可复用搜索入口”的结构前提，不需要新建 MCP 服务。
5. `ask_search.rs:451–545` 将该工具接口适配到同一个 `WebSearchTool`；`:262–312` 的 `SearchRun::execute` 读取原 `WebSearchConfig::load(credentials)`，按所选 provider 调用 `web_search::query_sources(config, query, RequestPolicy::Ask, on_request)`；`web_search/public_sources.rs:122–154` 仍是原免费两源或混合三源并发。因此 **源码已证明这些专项分支中的 Ask 与 PersonalAssistant 会消费同一份服务选择**，但本次未运行这些分支，不能宣称其真实安装版行为或模型答案验收通过。

### 在已验收 Ask 基线上移植本批搜索改动的最小范围

建议协调者先指定并验收上述一个现成 Ask 基线，再安排搜索改动移植；不要将整个 `07353f9c` 或 `d266ea89` 当作小工具桥提交合入当前发布基线。这些引用包含独立 Ask 框架、附件、上下文权限、原生目标及 Compact 等跨模块依赖，Core `create_standalone_ask` 还受 `ask-attachments` feature 控制，桌面 `src-tauri/Cargo.toml:88` 已启用此 feature。

在协调者指定的含依赖基线上，搜索侧应仅适配以下现有接口：

- 移植 `2870ab58` 的设置配置恢复、原 secret account、服务 ID、费用文案、聚合降级与来源去重规则；不要覆盖 Ask 分支已有 `RequestPolicy::Ask`、`on_request` 回调、页面读取权限与取消策略。`query_sources` 和 `public_sources::query` 的签名与当前搜索基线不同，需要按函数手工适配，不能整体替换文件。
- **额外删除 `ask_search.rs:280–285` 的 Hybrid 缺 key 提前返回**：该分支在 `web_search::query_sources` 外还有一层检查，单移植当前公共搜索模块的修复仍会让助手丢掉免费降级。需保留 Tavily/Brave/SearXNG 的原验证规则，不改变其他服务行为。
- 将新的 `SearchResult.providers` 保留到 `ask_search.rs:320` 的 `SearchSource` 和 `SearchEvidence`，必要时扩展 `qa_service/standalone.rs:284` 的 `AskSource` 及其事件映射、宿主 wire/前端类型；这些现有结构当前仅保留 URL/标题/摘要/读取状态，直接投影会丢提供方。来源链接当前已有，不要另造。
- 保留该 Ask 分支的证据边界：`ask_search.rs:562` 的 `usable_evidence` 要求页面已读且 excerpt 非空；若仅有搜索摘要、网页全被拒绝/读失败，返回 `SnippetOnly`，`assistant/tools.rs` 只向模型报告“摘要未核实”的状态。这与当前 legacy Ask 直接注入搜索摘要不同，不能把两条真实验收混记；在专项集成时需验证实际来源事件、摘要可用性和失败语义，不能把页面被拒绝伪装成读取成功。

可优先复用该分支已有测试：`ask_search::tests::{global_off_does_not_start_search,source_discovery_does_not_claim_blocked_reads_or_expose_page_bodies,cancellation_drops_a_request_waiting_for_its_response}`、`assistant::tests::{consecutive_tools_use_live_time_and_preserve_search_sources,cancellation_drops_tool_and_never_emits_completed}`，以及 `qa_service/standalone/model/tests.rs:149` 的本地 HTTP 工具循环。后者当前覆盖时间和计算，不证明免费/混合搜索的真实成功；应在相同模型/工具接口中补所选免费与 Hybrid 的来源回传、DeepSeek 失败降级、全空、取消及恢复配置检查。

这些测试本次仅核对源码，**未跨工作区重跑**。本分支 `c216c479` 的 legacy Ask Core fixture/live 证据仍仅适用于本分支；官方 DeepSeek 成功仍等待共用受控 CI。本次复核交付文档，不修改任何产品源码或其他分支。
