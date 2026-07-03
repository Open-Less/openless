# PR #716 — Overview Annual Activity Heatmap

## 概述

「今日概况」页面新增**年度活动热力图**，支持每日 / 每周 / 累计三种视图。引入**日活统计系统**（`activity_stats.json`）与历史记录解耦：概览页的今日指标、近 7 天趋势和年度热力图均以 `activity_stats.json` 为数据源，历史按 200 条上限管理的同时保证全年统计完整。

## 提交列表

| Commit | 类型 | 说明 |
|--------|------|------|
| `5647d1c` | feat(ui) | 添加年度活动热力图核心功能 |
| `7681d92` | feat(settings) | 取消历史保留限制（后重新调整） |
| `141de63` | docs(ui) | PR 截图 |
| `1fed45a` | docs(ui) | 设置截图 |
| `20435df` | merge | 合并 beta |

## 主要变更

### 1. 日活统计系统（新增）

**文件：** `persistence/activity_stats.rs`

写入时实时累加每日统计（次数 / 字数 / 时长），与 `history.json` 文字内容解耦。

**更新点：** 5 处 `append_with_retention` 调用后同步写入 `activity_stats.json`（dictation.rs ×3、qa_session.rs ×2）

**IPC：** `list_activity_stats` → `DailyActivityStat[]`

**前端基础设施：** `types.ts` + `ipc/history.ts` + `mock-data.ts`（365 天模拟数据）

### 2. 历史保留设置调整

| 设置项 | 默认值 | 说明 |
|--------|--------|------|
| `historyRetentionDays` | 0（不限） | 不按天数清理 |
| `historyMaxEntries` | 200 | 超出截断 |

`default_history_max_entries()` → `Some(200)`；`Wire` 层 `#[serde(default = "...")]` 同步更新。

### 3. 概览页数据源切换（本次改动）

`Overview.tsx` 中 **今日指标 / 近 7 天 / 年度热力图** 的数据源从 `history`（200 条上限）切换到 `activityStats`（永久保存）：

- 新增 `refreshActivityStats()` effect，调用 `listActivityStats` IPC
- `metrics` useMemo：优先读 `activityStats` 今日汇总 → fallback 到 `history`
- `weekly` useMemo：优先读 `activityStats` 近 7 天 → fallback 到 `history`
- `buildYearlyActivity` 签名改为接收 `DailyActivityStat[]` + `DictationSession[]` fallback
- **「今日累计记录」指标卡**：数值改为今日实际条数（`metrics.segmentsToday`），副标题根据是否超出本机存档上限动态切换文案
  - 超出 → "已超过本机存档最大个数"
  - 未超出 → "本机已存档 N 条"
- 「最近识别」列表仍使用 `history`（仅最近 200 条 / 最多 5 条展示）

### 4. Bugfix

`types.ts` 补齐 `showOverviewActivityHeatmap: boolean`（来自 `5647d1c` 的遗漏）。

## 文件变更

| 文件 | 操作 | 说明 |
|------|------|------|
| `persistence/activity_stats.rs` | + | 日活统计存储 |
| `persistence/mod.rs` | ~ | 注册模块 |
| `coordinator.rs` | ~ | `Inner` 集成 + 初始化 + 访问器 |
| `types.rs` | ~ | `default_history_max_entries()` |
| `dictation.rs` | ~ | 3 处写入后同步更新统计 |
| `qa_session.rs` | ~ | 2 处写入后同步更新统计 |
| `commands/history.rs` | ~ | `list_activity_stats` |
| `commands/mod.rs` | ~ | 导入 `DailyActivityStat` |
| `lib.rs` | ~ | 注册命令 |
| `types.ts` (TS) | ~ | `DailyActivityStat` + `showOverviewActivityHeatmap` |
| `ipc/history.ts` | ~ | `listActivityStats()` |
| `ipc/index.ts` | ~ | barrel 导出 |
| `mock-data.ts` | ~ | `mockActivityStats` + 默认值更新 |
| `Overview.tsx` | ~ | 全量数据源切换到 `activityStats` |

## 数据流

```
dictation → append_with_retention
  ├─ history.json（最新 200 条，超出截断）→ 「最近识别」列表
  └─ activity_stats.json（每日累加，永久保存）
       ↓ listActivityStats
      Overview.tsx
       ├─ metrics（今日指标卡片）
       ├─ weekly（近 7 天柱状图）
       └─ yearlyActivity（热力图三种模式）
```
