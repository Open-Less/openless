# OpenLess 设计反馈 — 全部要求

分支：`fix/design-feedback-cloud`  
已推送提交：`d04e6256`  
截图：`.handoff/shots/`  
验收环境：已安装的 dev App，系统 WKWebView。浏览器里量到的样式不能当成真机通过。

这条分支上更早的设计反馈提交已经动过热力图、按钮圆角、删除确认、通知颜色、侧栏选中态。这次新提交只加了分段控件的选中绘制，以及这份记录。下面「还没改」的部分留给后续自己做。

## 已经在云端分支里的改动

提交 `d04e6256`：

- 概览页「近 7 天 / 近 30 天」和指标切换改用共享组件 `Segmented`，不再用概览页自己的 `SegmentedToggle`。文件：`openless-all/app/src/pages/Overview.tsx`。
- 选中的分段选项钉在连续圆角外面：`clip-path: none`、`corner-shape: round`、圆角 `var(--ol-button-radius)`。文件：`openless-all/app/src/styles/continuous-corners.css`。
- 删掉只给旧概览分段用的 `.ol-overview-segment`。文件：`openless-all/app/src/styles/global.css`。
- 回归测试检查用量、识别管线、概览选中项可见，并且 `clip-path` 为 `none`、圆角 12px。文件：`openless-all/app/e2e/design-feedback.spec.ts`。

这条分支相对 `beta` 里更早已经在的相关改动（不是这次新写的，但云端分支上有）：

- 年度热力图换成 Rare UI 的 `GitHubActivity`（`openless-all/app/src/components/ui/GitHubActivity.tsx`、`github-activity.css`）。旧的自绘 `Heatmap.tsx` 已删。
- 文字/操作按钮圆角走 `--ol-button-radius: 12px`（`tokens.css`、`_atoms.tsx` 的 `Btn`、`global.css` 的 `.ol-action-button`）。
- 删除确认收成 `confirmDialog` + `ConfirmDialogHost`。
- 通知勾选色改向 `--ol-accent-solid-bg`。
- 侧栏选中用 `.ol-nav-btn-active`（字色 `--ol-ink`，底 `--ol-surface-2`）。

这些「已经改过」都还没有在已安装的 dev App 上复验。

## 还没改，按原话留着

### 1. 时间筛选要一致，选中必须看得见

用量页、概览页、设置里的用量，「近 7 天 / 近 30 天」外观要一致。选中项必须一眼可见。

证据：`shots/01-time-filter.webp`、`shots/02-period-7-30.webp`。

位置：`Usage.tsx` 的 `Segmented`；概览 `Overview.tsx` 的周期切换；`Segmented.tsx`；`global.css` 的 `.ol-segmented*`。

这次提交只让概览改用同一个组件，并钉住裁切。真机上选中项会不会整块消失，还没验。

### 2. 分段控件选中项整块不绘制

位置还在，文字和底色没有；没选中的选项正常。用量页选中「近 30 天」、识别管线选中「多模态模式」都出现过。

证据：`shots/02-period-7-30.webp`、`shots/07-pipeline.webp`。

位置：`Segmented.tsx`；`global.css` 约 1091 行起的 `.ol-segmented*`；`continuous-corners.css` 里 `@supports (corner-shape: superellipse(1.5))` 的 `clip-path: shape()` 和 `border-radius: 0 !important`。识别管线在 `settings/tabs.tsx`，class `ol-service-pipeline-track`。

真机是系统 WKWebView。Chromium 通过不算数。

### 3. 侧栏、标签、版本徽标的选中态

左侧导航、分段控件、标签页的选中项要有文字高亮和底色。DEV / Beta 版本徽标的高亮不能丢。

证据：`shots/03-sidebar-selection.webp`。

位置：`FloatingShell.tsx` 的 `.ol-nav-btn-active` 和品牌旁的 DEV/Beta 徽标；设置轨 `.ol-settings-rail-btn[aria-current='page']`；用量页页签在 `Usage.tsx`。

### 4. 概览年度热力图必须出现，并且不要自己重画

设置 → 外观与语言 →「概览页显示年度活动热力图」打开后，概览页必须出现年度热力图。

必须用 Rare UI GitHub Activity 的源码本体改数据，不要自己画格子。保留 11px 格子、3px 间距、28px 圆角、16px 内边距、周/月换算、月份标签、悬浮提示、底部可展开面板。数据从 GitHub 提交换成本地听写次数；仓库换成语音输入最多的软件（名字 + 次数）。卡片自然宽度约 771px，不要为了塞进窄栏把格子缩小。不许裁切、切边、标签错位。许可 MIT + Commons Clause，必须署名：设置 → 关于与更新 → 组件来源，以及 README。

证据：`shots/04-overview-missing.webp`、`shots/06-dialogs-heatmap.webp`、`shots/08-heatmap-detail.webp`。

位置：`ThemeSection.tsx` 的 `showOverviewActivityHeatmap`；`Overview.tsx` 的 `ActivityHeatmapCard`；`GitHubActivity.tsx`。

### 5. 热力图整块要么拉满下方，要么不要左右各空一截

指着 `div.ol-gha.ol-gha--with-panel.ol-col-12`（内联宽度 771px，组件 `GitHubActivity`）。整个部分要么把下方拉满，要么不要再左右各空出一截。

位置：`GitHubActivity.tsx` 里按列数算出的 `width`；`Overview.tsx` 里包着热力图的那一行 `flex: 0 0 auto`。

### 6. 概览是看板，整页不能滚动

这个页面按理不能滚动。看板的大小就是 App 默认展开后的大小。整页不能滚，能滚的只有看板里的内置容器（例如最近识别列表）。

位置：`Overview.tsx` 桌面端 `overflowY: 'auto'`（约 352 行）；`FloatingShell.tsx` 里概览页外层已经是 `overflow: hidden`，滚动发生在看板自己身上。

### 7. 通知和弹窗里的勾选按钮用主色

勾选按钮背景跟主色走，也就是关闭按钮那种蓝。要用 `--ol-accent-solid-bg`（浅色 `#2563eb` / 深色 `#3b82f6`）。不要用 `--ol-blue`（深色下是 `#74b7ff`）。

证据：`shots/05-notification-blue.webp`。

位置：`refinements.css` 的通知图标；`tokens.css` 的 `--ol-accent-solid-bg` 和 `--ol-blue`。

### 8. 按钮圆角只统一文字/操作按钮

只改文字/操作按钮为 `--ol-button-radius: 12px`。不动输入框、下拉框、弹窗容器、toast、开关、圆形图标按钮。不动 `--ol-radius: 16px`。

覆盖链：`Btn`（`_atoms.tsx`）输出 `.ol-pressable.ol-action-button` → `global.css` 的 `.ol-action-button` → `continuous-corners.css` 用 `border-radius: var(--ol-corner-radius, var(--ol-radius)) !important` 盖掉 inline。改 `--ol-corner-radius`，不要改 inline，也不要把 `--ol-corner-radius` 全局设成 12。

### 9. 删除确认要全站一个组件，并且看起来跟 App 是一套

所有 delete 确认弹窗共用一个组件、一套尺寸。

后来指着「删除渠道」弹窗（`Modal` > `ConfirmDialogHost`，`div.ol-dialog-card`，约 480×251）说：这和整体界面格格不入。字体大小和 App 里固定的字号都不一致。圆角也和 App 不对齐。这种弹框要作为组件在所有页面复用。

现有字号：标题 20px，说明 14.5px，危险时整段说明是红色。按钮混用了渠道删除按钮的描边样式。

位置：`ConfirmDialogHost.tsx`；`lib/confirmDialog.ts`；`global.css` 约 1964 行 `.ol-confirm-dialog*`；`Modal.tsx`；`overlays.css` 的 `--ol-dialog-radius`。

调用处已经包括：`ChannelList.tsx`、`History.tsx`、`Marketplace.tsx`、`LocalAsr/index.tsx`、`ExperimentalAsrSection.tsx`。

### 10. 识别管线三个选项必须看得见

设置 → AI 与模型 → 识别管线：传统模式 / 多模态模式 / DeepSeek 大统一。文字和选中态都要清楚。

证据：`shots/07-pipeline.webp`。

位置：`settings/tabs.tsx` 的 `Segmented`，class `ol-service-pipeline-track`。

### 11. 搜索展开后必须还是同一块

市集 / 词库工具栏的 `ExpandingSearch`，按钮 class `ol-search-icon`，无障碍名「搜索名称 / 描述 / 标签…」。点开之后表现不对：搜索的颜色和图标不一致，里面又出现一个搜索框。

搜索应该是一整套：整个搜索使用同一个颜色、同一个板块，只有展开和收起两种状态。

位置：`ExpandingSearch.tsx`；`global.css` 约 706 行 `.ol-search*`；约 2483 行 `input` 的 `background-color: var(--ol-control-muted)` 会把内层输入框再涂一层。用在 `Marketplace.tsx`、`Vocab.tsx`。

### 12. 模型拉取下拉的宽度和位置

设置里大一统卡片。选中的是 `button.ol-select-trigger`（`SelectLite` > `ProviderTools`），样式写了 `width: calc(100% - 38px)`，量到大约 21×36。框的大小和位置都不对。

它应该和「API 接口」「密钥接口地址」对齐：左边对齐这两个输入框的左边，右边对齐接口地址下面那一行 Temperature 输入框的右边。这个位置提过好几次。

位置：`ProvidersSection.tsx` 的 `ProviderTools`，`SelectLite` 的 inline 宽度；`global.css` 约 2387 行 `.ol-omni-tools-actions > .ol-channel-model-tools` 把子项拆成各占一半。Temperature 在 `.ol-omni-advanced-fields`，子项 `grid-column: span 6`（约 1331 行）。

### 13. 降噪强度要用正常滑动条

高级设置里 `input.ol-range`，无障碍名「降噪强度」，组件 `SettingRow` > `AudioDspSettings`。现在这条滑杆不是正常滑动条（轨道被输入框底色涂成双层）。用正常的滑动条：一条轨道，一个滑块。保持现在的最小、最大和数值行为。

位置：`AudioDspSettings.tsx` 的 `slider`；`global.css` 约 890 行 `.ol-range`，以及约 2483 行那条作用到所有 `input` 的底色。

### 14. 打包后的页面切换要接近网页

系统 WKWebView 里的体验要近似网页。现在切页有割裂感和卡顿，主要在页面切换。

位置：`TransitionViewport.tsx`；`refinements.css` 的 `.ol-transition-viewport`、`.ol-main-arrival`。

### 15. 本地只留一个已安装的 App

只保留 `/Applications/OpenLess.app`。不留其他 OpenLess `.app` / `.dmg` / 测试产物。聚焦搜索只能搜到应用程序里那一个。不要为了看效果再复制 DMG。

## 不要做的事

- 没有在已安装的 dev App 上验证过，不要写「已修复」。
- 不要改 `--ol-radius: 16px`。
- 不要自己重画热力图格子。
- 不要把 `--ol-corner-radius` 全局设成 12。
- 浏览器自动化通过，不等于系统 WKWebView 通过。
