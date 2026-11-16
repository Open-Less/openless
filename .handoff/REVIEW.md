# 设计反馈审查

对照 `.handoff/HANDOFF.md`、当前分支 `fix/design-feedback-round1`（`e417050a`）和本地分支。原交接文档保留不动。这里只记审查后仍然成立的结论。

真机验收仍然是已安装的 dev App（系统 WKWebView）。浏览器里量到的样式不能写成「已修复」。

## 问题现在的状态

| 编号 | 交接里的写法 | 审查后 |
|---|---|---|
| R1 | 选中分段整块不绘制，未解决 | 源码里已经有一层补丁：`global.css` 对 `button` 和 `.ol-segmented-option` 写了 `clip-path: none !important`。交接也写了当前 dist 在系统 WKWebView 里复现不出空白。补丁挂在 `:where()` 上，而 `continuous-corners.css` 的 `@supports` 仍把这些按钮放进 `clip-path: shape()`，并且后写的 `border-radius: 0 !important` 会盖掉非 important 的圆角。选中项有实心底色，被裁掉时文字和底色一起没；未选中项是透明底，看起来还在。这是第一项要改的代码。 |
| R2 | 对比度已改成 accent token | 只改了概览页自己的 `SegmentedToggle`。用量页走 `Segmented`，选中是 `--ol-ink` / `--ol-surface-2`。两条路径外观不一致，需求 1 没完成。 |
| R3 | 热力图开关已恢复 | 代码里概览页会渲染 `.ol-gha`。未在已安装 App 上复验。 |
| R4 | 已换 Rare UI 组件 | 不要重画格子。后续只核对署名、约 771px 自然宽度、以及有没有裁切。 |
| R5 | 勾选按钮已改 `--ol-accent-solid-bg` | 代码在 `refinements.css`。未在已安装 App 上复验。 |
| R6 | 按钮圆角已收成 12px | `--ol-button-radius` 是 12px，但分段选项仍可能被 `border-radius: 0 !important` 打成直角。不要改 `--ol-radius: 16px`，也不要动输入框、下拉、弹窗、toast、开关、圆形图标按钮。 |
| R7 | 删除确认已收成一个对话框 | 代码有 `confirmDialog`。未在已安装 App 上复验尺寸。 |
| R8 | 本地安装残留已清 | 不要为了验证再复制 DMG。也不要在复验前删掉 `/Applications/OpenLess.app`。 |

需求 2（侧栏选中字色、底色、DEV/Beta 徽标）在 `FloatingShell.tsx` 里有 `.ol-nav-btn-active` 和徽标。等分段控件改完再单独看，不和 R1 混在一次改动里。

## 当初那份 TODO 错在哪

1. 把交接表里的「已改」抄成「完成」。交接第六节写了：没在真机上验证就不能说已修复。
2. 把「相关分支都合并进来」当成修复步骤。`fix/pipeline-segment-alignment` 那一笔提交把操作按钮圆角设回 8px，运动实现也是旧的。合进来会退回需求 5。`origin/fix/overview-segment-radius` 已经在 `beta` 历史里，没有可合的新提交。
3. 把 R1 写成再改一次选中颜色。颜色改完之后空白仍然出现过；空白来自裁切，不是来自对比度。
4. 用 Chromium 的 Playwright 通过来关闭 R1。交接写明 Chromium 通过不等于系统 WKWebView 通过。
5. 把需求 9（清掉本机所有 App / DMG）排在真机复验之前。那样会拆掉唯一的验收环境。
6. 把热力图再画一版。需求 7 禁止自己重画。
7. 清理分支时连工作树和其他功能分支一起删。`appergb/settings-memory-updater`、`fix/review-startup-voice-edit`、`release/2.0.2-stable` 都有工作树。`main` 本地落后 `origin/main`，不能重置。

## 分支

本地只做两件事：

- 删除 `fix/macos-accessibility-guide-window`。它和 `beta` 是同一个提交，已经合入。
- 删除 `fix/pipeline-segment-alignment`。唯一多出来的提交会被当前分支的 12px 圆角和现行 `useSelectionMotion` 覆盖；合入是退步。远程引用留着，不推送删除。

其余本地分支保持不动，包括当前工作分支 `fix/design-feedback-round1`。不把未验证的设计改动合进本地 `beta`。

## 改动顺序

1. 分段选中项：从 shape 裁切里摘出来，圆角钉在 `--ol-button-radius`；概览页的近 7 天 / 近 30 天改用 `Segmented`。
2. 用现有回归测试锁住选中项可见、`clip-path: none`、圆角 12px。真机未复验前，文档状态保持「源码已改，未验收」。
3. 再逐条看侧栏选中态、热力图署名和宽度、通知主色、删除弹窗尺寸。每一条单独改，不打包。
