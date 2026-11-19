# 前端渲染统一与模块化重构（规划）

存放：`Dev-1`（三端统一平台以及对 UI 进行深度更改）。

这一支只暂存当前开发内容，不合并进 `beta` 或 `main`。基线是本地 `beta` `6d6ced54`，其中包含尚未推送到 `origin/beta` 的 52 个提交。状态：规划中，未开始改代码。

下面三份文档这次没有进工作区，不在本快照里补写：

| 文档 | 计划中的内容 | 本快照 |
|---|---|---|
| requirements.md | 要达成什么、验收标准、不做什么 | 未写入 |
| architecture.md | 渲染基线、Token、组件、浮层 API、平台适配、窗口与路由 | 未写入 |
| work-packages.md | 现状清单、旧→新文件映射、分包顺序与每包验收 | 未写入 |

## 回退点

动工前已打本地 tag（未推送）：

```text
backup/2026-11-17-pre-frontend-refactor/beta                                  6d6ced54
backup/2026-11-17-pre-frontend-refactor/dev/2.0.2-dev.5                       55c2a0f8
backup/2026-11-17-pre-frontend-refactor/cursor/design-feedback-remaining-d591 6d6ced54
backup/2026-11-17-pre-frontend-refactor/main                                  962705d5
```

回退：`git switch -C task/frontend-ui-unification backup/2026-11-17-pre-frontend-refactor/beta`。

注意：这 52 个提交（设计反馈 + 5 个合并分支）随 `Dev-1` 暂存在云端，不进入 `origin/beta`。以后若要进 `beta`，再单独决定走哪个 PR。

## 待拍板的决策

| 编号 | 问题 | 建议 |
|---|---|---|
| D1 | Linux 桌面现在是 `linux-egui`（原生 egui，约 4.2 万行 Rust，不走 WebView）。三端 UI 统一要不要把 Linux 主界面迁到 Tauri + WebKitGTK？ | 迁。React UI 成为三端唯一 UI；egui 保留到 Tauri Linux 达到功能对等再退役。胶囊/输入法链路（fcitx5）不动。 |
| D2 | Windows 窗口装饰：系统标题栏，还是与 macOS 一样无标题栏 + 前端自绘？ | 三端统一由前端 `AppFrame` 绘制内容区，标题栏策略由平台适配器给出 inset；Windows 用 `decorations:false` + 前端窗口按钮，Linux 保留系统装饰（Wayland CSD 不稳定）。 |
| D3 | 是否内置一款 Latin 字体保证三端字宽一致？ | 内置 Inter（OFL）作为 Latin/数字字体，CJK 回落系统字体。不内置也可，但需固定字重与行高。 |
| D4 | 最低内核基线 | macOS 14 WKWebView（Safari 17）∩ WebView2 Chromium 120 ∩ WebKitGTK 2.44 ∩ Android WebView 120 ∩ ArkWeb。架构文档尚未写入。 |
