# 鸿蒙主应用 UI v2：设计基线

> 目标：抛弃 v1 的「玻璃 + 光晕 + 模糊 + 描边 + 阴影」叠加式视觉，重新写一套自洽、
> 干净、可维护的原生 ArkUI 界面。旧代码在 `feat/harmony-native-ui` 分支保留作参照。

## 1. 原则

1. **内容优先**：底色是浅灰画布，内容是白卡；不用氛围渐变、不用大面积毛玻璃。
2. **一套强调色**：HarmonyOS 蓝 `#0A59F7`，与输入法键盘 `ime_accent` 对齐；状态色只有成功/警告/危险。
3. **层级靠色阶与字重，不靠阴影**：卡片用 1vp hairline 或 `card_alt` 色阶区分；阴影只给真正浮起来的层（半模态、对话框、悬浮键）。
4. **大标题 + 分组列表**：每个页面一个 30fp 大标题，内容按语义分组，组内用 56/72vp 行高与分隔线。
5. **动效克制**：120/180ms ease-out；按压 `scale 0.97` + 透明度；页面切换用系统 Navigation 转场，不手写转场。
6. **一套代码三档布局**：`sm`(<600) 底部标签、`md`(600–840) 图标窄栏、`lg`(840–1080) 文字侧栏、`xl`(>=1080) 文字侧栏 + 两栏内容。

## 2. token

| 类别 | 文件 | 规则 |
| --- | --- | --- |
| 颜色 | `common/Theme.ets` 的 `Palette`，值在 `resources/base|dark/element/color.json`（`c_*`） | 只引语义名，页面禁止 hex |
| 间距 | `Space`：4/8/12/16/24/32/40 | 8vp 栅格，小间距允许 4 |
| 圆角 | `Radius`：control 8 / card 12 / sheet 16 / pill 999 | — |
| 字号 | `Type`：pageTitle 30/38、section 20/26、cardTitle 17/24、body 16/24、label 14/20、caption 12/16、metric 40/46 | 只用这些档 |
| 动效 | `Motion.quick 120` / `Motion.base 180` / `Motion.slow 240` | — |
| 阴影 | `Elevation.float()` | 仅浮动层 |

内容宽度上限：`sm` 无上限（16 gutter）、`md` 640、`lg` 760、`xl` 880；两栏页面 `xl` 1200。

## 3. 组件库（`components/Kit.ets`）

| 组件 | 用途 |
| --- | --- |
| `PageScaffold` | 页面骨架：大标题 + 副标题 + 右侧操作槽 + 内容滚动区 + 统一 gutter/底部留白 |
| `Surface` | 白卡容器（radius 12，hairline，可选 `alt`/`plain` 色） |
| `Group` | 分组容器：内部行自动带分隔线（最后一行为 false） |
| `RowItem` | 标准行：可选 symbol 方块、标题/副标题、右侧 value / chevron / 自定义槽 |
| `SectionTitle` | 卡片内分区标题 |
| `PrimaryButton` / `TonalButton` / `TextButton` / `IconButton` | 四档按钮，禁止再手写按钮样式 |
| `Segmented` | 单选分段/胶囊组（内部自动换行，不允许溢出） |
| `Tag` | 只读标签（状态/模式） |
| `SearchField` | 搜索输入 |
| `EmptyHint` / `Skeleton` / `ErrorNote` / `Banner` | 空态/骨架/错误/提示条 |
| `MetricBlock` | 数字 + 单位 + 说明，数字档 `metric` 或 `section` |
| `SettingSwitch` | 标题 + 说明 + 开关 |
| `PageBackBar` | 二级页返回条（不要每个页面手写） |

## 4. 页面清单（本轮范围：主程序）

概览 / 历史 / 词典 / 风格 / 设置 + 二级页（凭据、市场、纠正规则、翻译、问答、速记、模型设置）。
输入法面板本轮不动，下一轮再统一到同一套 token。

## 5. 约束

- 业务逻辑与 `data/Api.ets` 的接线保持不变；只重写视图与局部状态组织。
- 每个页面同一结构名（如 `export struct OverviewPage`），`AppShell` 路由表不变。
- 改完必须能 `bash openless-all/app/scripts/build-harmony-app.sh` 构建通过；契约测试在整轮结束后统一更新。
- 页面内禁止出现颜色 hex、字号字面量、手写按钮/行样式；一律走 `Kit` + `Theme`。
