# 供应商渠道卡片化 实施计划

> 状态：P0 已完成（2026-08-07，PR #918）
> 日期：2026-08-04
> 范围：设置 → AI 提供商，LLM 润色 + ASR 语音转写
> 参考：[Calcium-Ion/new-api](https://github.com/Calcium-Ion/new-api) 的 Channel 模型与重试策略

## 1. 要解决的问题

今天一个供应商只能存一份配置（一把 key、一个 endpoint、一个模型）。实际使用中：

1. **同一家有多把 key**（主号 / 备号 / 白嫖号），现在只能存一把，换 key 靠手动覆盖粘贴
2. **key 之间要频繁切换**，切换过程中旧配置就丢了
3. 某把 key 被限流（429）时没有任何自动应对，整条润色链路直接失败

目标：把配置从"一个供应商一个槽"变成"一张张可命名、可排序、可开关的卡片"，并让失败能自动顺延到下一张卡片。

## 2. 现状核对

| 事实 | 位置 |
| --- | --- |
| 存储层已经是 `HashMap<String, Entry>`，key 是 preset id | `credentials.rs:169` |
| `CredsLlmEntry` 已有 `displayName` 字段，前端从未使用 | `credentials.rs:257` |
| ASR 凭据按 provider 隔离正确（空槽才填默认值） | `ProvidersSection.tsx:370` |
| LLM 切 preset 会**强制覆盖** endpoint/model，注释所述的"共用槽"bug 早已不成立 | `ProvidersSection.tsx:305` |
| 全局零重试 / 零故障转移（`rg retry\|backoff\|fallback` 无命中） | — |
| 凭据读取是**隐式全局** `CredentialsVault::get(...)` 去查 `root.active.*`，调用方无法指定渠道 | coordinator.rs / commands/providers.rs 共数十处 |
| ASR provider id 同时承担**协议路由 key**（百炼一个 id 分三协议，stepfun 分两协议） | `coordinator.rs:341` |
| 新手引导直接嵌 `<ProvidersSection kind="asr" />` | `Onboarding.tsx:208` |
| Windows 默认 ASR 是本地 Foundry（无需 key，开箱即用） | `credentials.rs:155` |
| LLM 单次请求超时 30s | `polish.rs:23` |

**结论**：存储结构不用推倒，改 key 语义即可；真正的成本在"凭据显式化"这次重构。

## 3. 已定的设计决策

| 决策 | 结论 |
| --- | --- |
| 范围 | LLM 与 ASR **都**做卡片 |
| 排序 | 列表可拖拽，越靠上越优先；启用列表的**第一个 = 当前使用** |
| 开关 | 打开 = 加入重试队列；**关掉自动沉到列表末尾**；重新打开回到启用组末尾 |
| 触发切换 | 429 等错误**立即**切下一个渠道 |
| 超时 | **不触发**切换（本期先这样） |
| 渠道失败 | **只在卡片上标红**（如「上次失败 · 401 · 3 分钟前」），**不自动禁用** |
| 全部失败 | 润色链路降级为**直接插入 ASR 原文** + 右上角提示 |
| 特殊项 | 本地引擎（qwen3 / sherpa / Apple 语音 / Foundry）与 Codex OAuth **不做预置固定卡片**，它们是「＋添加渠道」供应商下拉里的普通选项，选中即长出卡片，表单里没有 key/地址字段 |

### 3.1 ASR 与 LLM 语义统一

429 只出现在**建连 / 鉴权阶段**——此时一个字都还没吐出来，音频缓冲尚未被消费，换渠道重连是安全的。因此两边共用同一套心智：

> 排序 = 优先级；开关 = 在不在重试队列；失败（非超时）顺延下一个。

ASR 唯一的额外规则：**一旦开始出字就不再切换**，之后连接断了就是断了（流式已吐字，回滚会造成文字重复或跳变）。

### 3.2 429 冷却（必须有）

若不加冷却，限流期间**每一次**听写都会白赔一次「打 1 号 → 429 → 打 2 号」的往返（数百毫秒，同步链路里能感知）。

- 渠道返回 429 → 打 **60 秒冷却**，冷却期内直接跳过
- 冷却是**内存态**，不落盘，重启即清
- 卡片上显示「限流中 · 47s」小字，到期自动恢复，无需用户干预

### 3.3 超时值下调（独立改动）

保留"超时不切换"的规则，但把 `DEFAULT_REQUEST_TIMEOUT_SECS` 从 **30s 压到 8s**。润色是用户盯着屏幕等的同步链路，8 秒未返回的渠道等下去没有意义。

## 4. 数据模型

```rust
struct Channel {
    id: String,             // 迁移沿用 preset id；同厂商新卡按 -2 / -3 分配独立 id
    name: String,           // 用户取的名字，如「硅基流动-主号」
    provider_type: String,  // deepseek / volcengine / sherpa-onnx-local / codex_oauth ...
                            // 决定协议路由 + 表单形状，必须独立于 id
    enabled: bool,
    order: u32,             // 拖拽排序；关掉时自动置到末尾
    last_error: Option<ChannelError>,  // { kind, message, at } —— 卡片标红用
    last_test: Option<ChannelTest>,    // { ok, latency_ms, at } —— 连通测试结果
    // 凭据字段沿用现有 CredsAsrEntry / CredsLlmEntry，按 provider_type 决定渲染哪些
}

// 仅内存，不落盘
struct ChannelRuntime {
    cooldown_until: Option<Instant>,   // 429 临时冷却
}
```

**`provider_type` 必须独立于 `id`**：否则 `coordinator.rs:341` 那条"按 provider id + 模型名路由到具体协议实现"的链会断——这是漏了就整个 ASR 挂掉的点。

`active.llm` / `active.asr` 不再是用户直接选择的第二份真相，而是由排序与开关同步计算的
**兼容缓存**；旧主链路仍读取它们，"当前使用"始终等于启用列表的第一个。

## 5. 迁移

1. 遍历现有 `providers.llm` / `providers.asr` 的每个非空 entry，各补齐渠道元信息
   - `id` 沿用原 map key，`provider_type` = 原 map key，`name` = `displayName` 或 preset 显示名
   - 新建同厂商的第二、第三张卡片使用 `<preset>-2`、`<preset>-3`，不改动迁移前的凭据 key
2. 原 `active.llm` / `active.asr` 指向的那张排到 **order = 0**，其余按 ASR_PRESETS / LLM_PRESETS 原顺序跟随
3. 全部默认 `enabled = true`
4. **全新安装**（无任何 entry）：按平台预置
   - Windows → 一张 Foundry 本地 ASR 卡片（保住开箱即用）
   - mac / Linux → 不预置，走引导
5. 迁移必须幂等，且失败时保留原 JSON 不动（参考现有 `load_credentials_for_update` 的写法）

## 6. 重试策略

> **状态：未实现，属于 P2。** P0 里一次失败就是一次失败，不会换渠道。
> 下表是已定但**尚未落地**的目标行为。

### 6.0 代码里已经存在的两样东西（别和渠道故障转移混为一谈）

排查时容易在代码里搜到 `retry` 就以为做了，这两处都是**既有代码**，与渠道无关：

1. **`net.rs::send_with_retry` —— 连接层重连，不是渠道切换。**
   只对 `err.is_connect()`（TCP 握手被拒 / 连接重置，请求**尚未送达**服务端）重试，
   150/300/600/900ms 退避。**拿到任何 HTTP 响应就直接返回**（含 429/401/5xx），
   超时明确不重试。它重连的始终是同一个 endpoint，永远不会换到另一张卡片。

2. **润色失败已经会回落 ASR 原文。**
   `coordinator/polish_flow.rs::polish_or_passthrough` 的失败分支：
   ```rust
   Err(e) => {
       log::error!("[coord] polish failed, falling back to raw: {reason}");
       (raw.text.clone(), Some(reason))
   }
   ```
   也就是说，「全部渠道试完仍失败 → 插入 ASR 原文」这条决策**天然满足**，
   P2 要做的只是在回落之前多试几张卡片，而不是新建一条兜底路径。

### 6.1 目标行为（P2）

照搬 New API `shouldRetry()` 的分类，按桌面场景裁剪：

| 情况 | 行为 |
| --- | --- |
| 429 | **切下一个** + 当前渠道 60s 冷却 |
| 401 / 403 | **切下一个** + 卡片标红（不自动禁用） |
| 5xx / 连接失败 | **切下一个** + 卡片标红 |
| 超时 | **不切**，直接失败（本期决策） |
| 400 参数错误 | **不切**（换渠道多半是同样的错） |
| 2xx | 成功 |
| 全部启用渠道试完仍失败 | 插入 ASR 原文 + 提示 |

**不抄** New API 的：`Weight` 加权负载均衡（单用户无负载可均衡，随机选渠道反而让"在用哪个"不可预测）、`Group` / `UsedQuota` / `Balance`（多租户计费概念）、`AutoBan`（桌面软件静默关用户配置会让人一脸懵）。

**缓一缓**：`ModelMapping` / `ParamOverride`，有用但非第一版必需。

## 7. UI

```
┌─ LLM 润色 ──────────────────────────────┐
│ ⠿ ● 硅基流动-主号   deepseek-v4   28ms   ⋮ │  ← 生效中
│ ⠿ ○ Ark-备用       deepseek-v3-2  —      ⋮ │  ← 备用
│ ⠿ ○ 阶跃星辰       (限流中 · 47s)         ⋮ │  ← 429 冷却
│ ⠿ ⊘ OpenAI        (上次失败 · 401)       ⋮ │  ← 已关闭，沉底
│ ＋ 添加渠道                                │
└──────────────────────────────────────────┘
```

添加/编辑弹窗：名字 → 选供应商（自动填 baseUrl / 模型占位）→ 按 `provider_type` 渲染凭据字段
→ 「测试连通」；字段自动保存，关闭只负责退出弹窗。

可复用的现成件：
- `validateProviderCredentials` / `listProviderModels`（`ProvidersSection.tsx:854` 起）
- 按 provider 分支渲染凭据字段的逻辑（火山双鉴权模式、讯飞双字段、百炼词表等）
- 本地引擎卡片不显示凭据字段；模型下载与切换继续集中在「高级 → 本地模型」，避免两处管理同一份模型状态

**新手引导**：列表为空时直接摊开添加表单，跳过空态与加号，省一次点击。

**平台过滤**：macOS 只显示 Qwen3 Local / Apple Speech；Windows 只显示 Foundry /
Sherpa；Linux 与 Android 不显示这些桌面专有本地引擎。云端供应商全平台可选。

**草稿回收**：保持自动保存。只有打开后从未发生任何用户交互的草稿会在关闭时回收；
改过名字、供应商、凭据、模型，或执行过验证/模型拉取后都必须保留，即使内容后来清空
或异步保存失败。这样无凭据的本地引擎 / Apple Speech / Codex OAuth 也能正常创建，且
关闭弹窗不会与 blur/debounce 保存竞争删除卡片。

## 8. 分期

| 期 | 内容 | 可否独立发布 |
| --- | --- | --- |
| **P0** | 渠道数据模型 + 迁移 + 卡片 UI + 拖拽排序 + 测试连通。**不做重试** | ✅ 独立故事：「我有两把 key，想随手切」 |
| | ↑ 已完成（PR #918）。**此时多渠道的价值是"存档 + 手动切换"，不是自动容错**：排在第二的卡片永远不会被自动用上，要用得手动拖到第一位。 | |
| **P1** | 凭据显式化重构：`CredentialsVault::get(...)` → 上层解析 `ResolvedChannel` 显式下传 | ❌ 纯重构，无用户可见变化，P2 前提 |
| **P2** | 重试 + 故障转移 + 429 冷却 + 超时下调 + 全挂兜底 | ✅ |

P1 是本需求最大的单块工作量，比卡片 UI 大得多。P1 + P2 合成第二个 PR。

## 9. 待确认

- [x] 拖拽排序：手写 pointer 事件（已定）。Tauri webview 默认 `dragDropEnabled` 会吞掉
      HTML5 的 `dragstart`/`drop`，`draggable` 在打包后的 app 里不触发；pointer 方案
      Windows / Android 行为一致，并配合「拖拽结束吞掉补发 click」避免关掉设置弹窗。
- [x] 卡片列表的移动端（Android）形态：与桌面共用同一渠道 UI；Android 走
      `load_credentials` 的同一条迁移路径，无需单独实现。
- [x] Android 加密信封：未改版本号。v2 载荷新增的 `providerType`/`order`/`enabled`/
      `lastTest` 均为 `Option` 或带默认值字段，老版本 serde 忽略未知字段，可降级读取。
- [x] P0 验收判据：已满足（同供应商多卡、重启后顺序与内容不丢、拖拽后第一张生效），
      由 `persistence::credentials` 迁移/排序测试与作者 macOS 实机验证覆盖。
