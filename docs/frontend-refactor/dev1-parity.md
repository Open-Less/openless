# Dev-1 前端合并清单

验收基线是 `Dev-1` 的 `c45430f4`，保留其已有宿主、业务和 UI。结构参考 `frontend/react-refactor` 的 `a5ade0a7`。两者共同祖先为 `d31c3d2c`；合并工作树的第一父提交是 `Dev-1`，初步冲突选择只是落位用的脚手架，不构成交付。原草稿 PR #1450 的 beta 历史不改写、不自动带入 Dev-1 的未发布功能。

| 对照项 | Dev-1 来源 | 合并责任与约束 | 验收 |
|---|---|---|---|
| 今日概览：近期、日周期、年度底部 | `81e5d6df` / `806376e5` / `7fb17c61` / `6d6ced54` 的 Overview、GitHubActivity 和 overview 样式 | Visual：在 canonical overview 与共享历史快照恢复视觉、状态缓存和真实年度应用统计。保留原生 `get_activity_apps` 命令和平台处理。 | 相同数据下对照 1300×835、最低允许内容区、三档缩放；检查年度区、应用排行、最近识别、历史写入后更新，以及热力图 11px/3px 和署名。 |
| 确认、提醒、删除反馈、通知 | `d3fbbbb6` / `205579c7` / `6d6ced54` 的 ConfirmDialog、NotificationCenter、SavedToast 和对应样式 | Visual：Dev-1 画面和操作语义接入重构后的每窗 interaction owner；只保留一个提示/确认队列、作用域结算和焦点恢复。 | 确认/取消/Escape、替换/离开、成功/失败、退出期间按钮和文字完整，不能重复通知。 |
| 网格、控件、选中视觉及切页 | `e22cc6b7` / `e66dbeda` / `d04e6256` / `205579c7` / `6d6ced54` 的 grid、选择、导航、提示、百分比进度 | Visual：共享样式和组件；Settings 仅改自己消费端，Runtime 接下载/模型。保留单坐标空间和明确的排版变体。 | 核对浅暗色、选中、短/长词、过渡时点击、表格与提示层对齐。 |
| DeepSeek 大统一管线、模型/凭据与网页搜索 | `18573ba5` → `f26baa59` 及 `822a2337` 的 settings、provider 模块和 locale | DeepSeek：canonical settings/providers/QA，通过 typed API 和现有命令；保留三种真实管线与官网 DeepSeek 凭据约束，不恢复旧整页 IPC。独占所有 locale 合并。 | 进入设置可见三种管线，能实际选择与保存；核对失败草稿、模型选择、WebSearch 与 Harmony 原有能力限制。 |
| QA 会话、工具、来源、朗读与底部 | `18573ba5`、`97ce4475`、`abd0b237` 等 QA/朗读及来源模块 | DeepSeek：canonical QA，保留窗口测量、会话代次、Android 内嵌和作用域处理；Runtime 只碰装配或宿主适配。 | 搜索、来源、朗读取消/重进、保留会话、长答案滚动和实际可绘制边界。 |
| 权限引导、学习与胶囊 | `941fe673`、`d17882dd`、`abd0b237`、`f3b82635`、`42958c2a`、`c811d9f2`、`0a8dbf82` | Runtime：将有效前端行为接入 canonical app/feature/runtime；不改 Dev-1 已有后端。保留 beta 已落地的 Windows 学词卡位置和退役开关移除。 | 启动门禁、恢复与热键、权限页、胶囊语言菜单、保护性学习、短录音、会话替换与工作区边缘。 |
| 模型下载、设置细节、内容页、更新 | `81e5d6df`、`205579c7`、`6d6ced54` 和对应域差异 | Runtime：本地模型、下载、更新、云同步及内容页；DeepSeek 负责 settings/provider 对接；Visual 负责共享控制与主题。保留重构的写队列、下载订阅、作用域反馈。 | 长内容下面板可操作；下载/更新确认、失败、回退；草稿、搜索、数据写入和本地模型动作。 |
| 平台、CI 与打包 | Dev-1 native/core 与 Harmony、后来的 beta 四笔提交，以及 `a5ade0a7` 修复 | 合并：核对命令、生成 audit、真实入口与平台差异；保留 ASR 测试数据保留期修复。按新基线、实施合并和实测证据区分结果。 | npm 全量、Chromium/WebKit、Harmony/HAP 契约、安装版 Tauri；用户确认实测后才打 DMG。 |
