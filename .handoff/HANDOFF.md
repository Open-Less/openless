# OpenLess 设计反馈 — 交接提示词

> 交给接手的人/Agent 直接开工。截图在 `shots/`。
> 仓库 `Open-Less/openless`，分支 `fix/design-feedback-round1`，HEAD `e417050a`（`2.0.2-dev.4+build.20261115`）。
> 验收环境：云端 `workflow_dispatch` 构建的 **dev** DMG（`release-tauri.yml`，`platform=macos`），不是 Beta/Release。
> 真机引擎是 **系统 WKWebView**；Chromium 通过 ≠ 真机通过。

## 一、用户需求（验收清单）

1. **时间筛选一致**：`近 7 天 / 近 30 天`（用量页、概览页、设置-用量）外观一致，选中项必须一眼可见。
2. **选中态必须可见**：左侧导航、分段控件、标签页的选中项要有文字高亮/底色；版本徽标（DEV/Beta）高亮不能丢。
3. **概览页控件齐全**：`设置 → 外观与语言 → 概览页显示年度活动热力图` 打开后，概览页必须出现年度热力图。
4. **主色蓝统一**：通知/弹窗里的勾选按钮背景用主色，即 `--ol-accent-solid-bg`（浅色 `#2563eb` / 深色 `#3b82f6`）；**不要**用 `--ol-blue`（深色下是浅蓝 `#74b7ff`）。
5. **按钮圆角统一 12px**：
   - 只改文字/操作按钮 → `--ol-button-radius: 12px`；
   - **不动**输入框、下拉框、弹窗容器、toast、开关、圆形图标按钮（16px / 999px / 20px 保持不变）；
   - **不动** `--ol-radius: 16px`；
   - 覆盖链（不按这个改会被 `!important` 吃掉）：`Btn`（`src/pages/_atoms.tsx`）输出 `.ol-pressable.ol-action-button` → `global.css` 的 `.ol-action-button { --ol-corner-radius: 8px }` 钉死 → `continuous-corners.css:143` 用 `border-radius: var(--ol-corner-radius, var(--ol-radius)) !important` 覆盖 inline。**改 `--ol-corner-radius`**，不要改 inline，也不要把 `--ol-corner-radius` 全局设成 12（会连带改掉下拉/输入框内嵌圆角）。
6. **弹窗复用同一组件**：所有 delete 确认弹窗共用**一个**组件、一套尺寸，不要"有的太大有的太小"。
7. **年度热力图必须用给定开源组件的源码本体**：Rare UI GitHub Activity（页面 `https://rareui.com/components/githubactivity`，源 `https://raw.githubusercontent.com/swamimalode07/rare-ui/main/public/r/github-activity.json`）。做法是**把源码扒下来改一改**，不是自己画：
   - 保留原几何：11px 格子、3px 间距、28px 圆角、16px 内边距、周/月换算、顶部月份标签、悬浮 tooltip、底部可展开面板；
   - 只换数据源：GitHub 提交 → 本地听写次数；仓库 → 语音输入最多的软件（名字 + 次数）；
   - 卡片需要自然宽度（≈771px），**不要**为塞进窄栏缩小格子；
   - 不许出现裁切、切边、标签错位；
   - 许可 MIT + Commons Clause，**必须署名**：设置 → 关于与更新 → 组件来源，以及 README。
8. **识别管线选项必须可见**：`设置 → AI 与模型 → 识别管线` 的 `传统模式 / 多模态模式 / DeepSeek 大统一`，三个选项的文字和选中态都要清楚可见。
9. **本地环境干净**：只保留 `/Applications/OpenLess.app`。不留任何 OpenLess `.app` / `.dmg` / 测试产物；聚焦搜索只能搜到「应用程序」那一个。

## 二、已复现的问题（只列这些，均带截图/实测）

| 编号 | 现象 | 证据 | 现状 |
|---|---|---|---|
| **R1** | **分段控件的选中项整块不绘制**：位置仍占着，但没有文字、没有底色；未选中的两个反而正常 | `shots/07-pipeline.webp`：识别管线三个槽位，中间 `多模态模式`（当前选中）是空白；`shots/02-period-7-30.webp`：用量页只剩 `近 7 天`，`近 30 天`（当前选中）消失 | **未解决**。dev.3 里 `.ol-segmented-option[aria-pressed='true']` 已经是 `color: var(--ol-ink); background: var(--ol-surface-2)` 这种正常色，仍然发生；当前 dist 在**系统 WKWebView** 里量不到同样现象（三个选项都在，`rgb(250,250,250)` / `rgb(125,125,134)`）。必须在**已安装的 dev App** 上取证：把该按钮的 `getComputedStyle`（color/background/opacity/visibility/clip-path/corner-shape/rect）临时写到 `~/Library/Logs/OpenLess/openless.log` 再复现 |
| **R2** | 分段控件选中项**对比度过低**：白字压 `--ol-blue`，深色下 `#74b7ff` + `#fff` | 真机 WKWebView 实测 `color rgb(255,255,255)` / `background rgb(116,183,255)`；代码在 `Overview.tsx` 的 `SegmentedToggle` inline style | 已改 `--ol-accent-solid-bg` + `--ol-accent-solid-ink` |
| **R3** | 概览页**年度热力图开关打开也不显示** | `Overview.tsx` 渲染分支被删（判断在、卡片不在） | 已恢复 |
| **R4** | 年度热力图**越改越丑且被裁**：手绘版把 53 周压进窄栏（格子≈4px）、月份标签错位、卡片被容器裁切 | `shots/06-dialogs-heatmap.webp`、`shots/08-heatmap-detail.webp`；旧实现 `src/components/Heatmap.tsx` 自绘 | 已改用 Rare UI 官方组件 `src/components/ui/GitHubActivity.tsx`（11px 格子、独占整行、可展开面板） |
| **R5** | 通知/弹窗**勾选按钮蓝色不对**（浅蓝而非主色） | `refinements.css` 用的是 `--ol-blue`；`shots/05-notification-blue.webp` | 已改 `--ol-accent-solid-bg` |
| **R6** | 按钮圆角**三套混用**（8 / 12.5 / 16px） | 实测 `.ol-correction-add` 8px、`.ol-segmented-option` 12.5px、其余 16px；根因见需求 5 | 已统一 `--ol-button-radius: 12px` |
| **R7** | **delete 弹窗各自实现**、尺寸不一 | `ChannelList`、`ExperimentalAsrSection`、`LocalAsr`、`Marketplace`、`History` 各自 `window.confirm` / 自定义 modal；`shots/06-dialogs-heatmap.webp` | 已收敛到 `confirmDialog` + `ConfirmDialogHost`（480px，z-1200） |
| **R8** | 本地**残留大量 App / DMG / 测试产物** | 全盘扫描：9 个 DMG（含废纸篓）、`OpenLessTest.app`、`src-tauri/target` 约 19G、Playwright 产物 | 已清理，只剩 `/Applications/OpenLess.app` |

## 三、关键代码位置

- token：`src/styles/tokens.css`（`--ol-radius: 16px`、`--ol-button-radius: 12px`、`--ol-accent-solid-bg/-ink`、`--ol-blue`）
- 圆角 `!important` 覆盖链：`src/styles/continuous-corners.css:143`
- 按钮原子：`src/pages/_atoms.tsx`（`Btn` → `.ol-pressable.ol-action-button`）
- 分段控件：`src/components/ui/Segmented.tsx` + `global.css` 的 `.ol-segmented*`；指示器动画 `src/lib/motion.ts` 的 `useSelectionMotion`
- 概览页：`src/pages/Overview.tsx`（`SegmentedToggle`、`ActivityHeatmapCard`）
- 热力图：`src/components/ui/GitHubActivity.tsx` + `github-activity.css`
- 弹窗：`src/lib/confirmDialog.ts` + `src/components/ConfirmDialogHost.tsx`
- 通知：`src/lib/notifications.ts`、`src/components/NotificationCenter.tsx`、`refinements.css`

## 四、验收方式

1. 云端 dev 构建 → DMG → 装真机，**在真机 WKWebView 里量 computed style + 截图**；不要只用浏览器自动化下结论。
2. 逐条对照需求 1–9，尤其是 **R1**：必须在已安装 App 上确认三个选项（管线/时间筛选）都能看见。
3. 回归基线：`npx playwright test --project=chromium`（162 passed）、`--project=webkit`（160 passed，1 个 flake）、`npm run build`、`npm run format:check`。
4. 交付后清掉本地产物，聚焦搜索只留 `/Applications/OpenLess.app`。

## 五、用户截图

| 文件 | 对应用户原话 |
|---|---|
| `shots/01-time-filter.webp` | 「时间筛选组件状态不一致……部分页面显示正常，部分页面出现异常或缺失」 |
| `shots/02-period-7-30.webp` | 「Usage 里切换近7天/近30天，选中的选项完全看不见」 |
| `shots/03-sidebar-selection.webp` | 「左侧按钮切换时选中项应有文字高亮及版本标识高亮」 |
| `shots/04-overview-missing.webp` | 「概览页原本应有的 UI 元素/功能控件未显示」 |
| `shots/05-notification-blue.webp` | 「弹出勾选按钮的背景颜色应该同步主色调，也就是类似关闭按钮的那种蓝色。你这里的蓝色也不对」 |
| `shots/06-dialogs-heatmap.webp` | 「有些弹窗太大有些太小……所有 delete 弹窗共用一个组件」「打开了年度热力图还是不显示」 |
| `shots/07-pipeline.webp` | 「我选择了多模态，但多模态目标偏置完全不可见」「包括这个页面的近七天、近三十天统计」 |
| `shots/08-heatmap-detail.webp` | 「And there.」——年度热力图细节 |

## 六、不要做的事

- 不要为「看着对不对」造测试 App / 复制 DMG；真机问题用真机引擎量样式或写日志判断，用完立刻清掉。
- 不要改 `--ol-radius: 16px`，不要动非按钮控件的圆角。
- 不要自己重画热力图。
- 没有在真机上验证过就别说「已修复」。
