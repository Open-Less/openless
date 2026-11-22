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
| Agent consumer 核查 | `crates/openless-core/src/coding_agent.rs` 的 Claude CLI 工具白名单含 `WebSearch`，Codex 流事件解析 `web_search` | 这些是外部 CLI 自己的搜索及工具事件，不调用本地 `web_search::search`，不受本设置控制。本次没有伪造新的 Agent 系统或接管外部 CLI 工具；其联网可用性未验收 |
| 并发、部分失败、全空 | `crates/openless-core/src/web_search/public_sources.rs::query/Source::query/merge` | 免费 fan-out 两源，混合三源；每源错误/拒绝/超时独立处理。有有效结果就返回其余结果，混合缺 key 仅跳过 DeepSeek；全空返回明确错误，全失败返回来源错误。CAPTCHA HTML、JSON error、HTTP 非成功不伪装成空成功 |
| 排序与溯源 | 同文件 `merge`、`web_search.rs::SearchResult/context` | 按来源固定顺序、名次轮转，去重仅忽略 URL fragment，不擅自删除有语义的查询参数。结果包含原始可引用 HTTP(S) 链接和 `providers`；共同命中合并提供方。不保证搜索结果本身正确或实时 |
| 取消与超时 | `cloud_providers.rs::qa_search_context`、`web_search.rs::search/search_inner` | 共享入口每 25ms 检查原取消信号，取消时丢弃搜索 future 及未完成请求；无需新会话 ID。总搜索最多 15 秒、源请求最多 8 秒、最多前三页摘录各 5 秒并发。正文读取失败保留检索摘要 |
| 独立流程保持 | 改动只在上述搜索模块及搜索设置 | 普通听写、Shift 翻译、浏览器地址栏搜索没有改动；未把它们导入搜索设置，未运行这些独立功能的硬件回归 |

### 配置兼容与同步限制

当前新版 UI 保存并重开后恢复各服务配置；旧版仍能读取顶层活动服务。**旧版搜索设置 UI 只重建三个顶层字段，旧版编辑并保存会删除 `providerConfigs`**，因此配置集合恢复要求使用包含本修改的客户端，不能承诺跨旧版编辑保留。没有对已丢失的历史 key 做虚构恢复。

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
