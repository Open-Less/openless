# Android 输入法 UI 重写设计与实施记录

> 状态：本地重写与自动化回归已进入手机验收阶段。最初的设计目标与后续实现、验证证据分节记录，不能把设计提案当成已实现功能。
> 基线：仓库 `2d9fba66`。前半部分保留改造前的静态定位，后半部分记录实际实施与测试。历史行号不是当前源码行号；源码规模不作为质量判断。

## 1. 目标与非目标

**目标**：让输入法的展示、手势、候选呈现、模式切换、尺寸主题、生命周期状态可独立测试与替换，同时不改变用户已依赖的输入行为与数据。

**非目标**：滑行输入、皮肤商店；词库/Rime 引擎改造；词建议质量提升。**不把技术栈迁移作为本次目标**——暂沿现有 Kotlin Views 实现以减少变量，是否迁移另行评估。

**外部参考的性质**：下列项目只作"功能声明"级别的参考，README 不能证明其实现成熟或经过验证，本规格不转述其代码细节。

- [HeliBoard](https://github.com/Helium314/HeliBoard)：可定制布局、单手模式、明暗主题；
- [FlorisBoard](https://github.com/florisboard/florisboard)：剪贴板面板、主题机制；**其词建议仍在开发，不作为基线**；
- [Fcitx5 for Android](https://github.com/fcitx5-android/fcitx5-android)：按键弹出预览、长按符号、展开候选；
- [Trime](https://github.com/osfans/trime)：Rime 引擎与前端渲染分离。

## 2. 改造前现状（历史静态抽样，非穷尽审计）

| 事实 | 依据 |
|---|---|
| 主界面主要由单个 `InputMethodService` 构建 | [OpenLessImeService.kt](kotlin/OpenLessImeService.kt)（6591 行） |
| 笔画面板由其控制器参与构建 | [StrokeInputController.kt](kotlin/StrokeInputController.kt) `buildStrokeView` L129 |
| 四模式：语音/笔画/剪贴板/英文 | `OpenLessImeService.kt` L36 |
| 英文三层：字母/数字/符号 | 同上 L39–L42 |
| 拉丁双语义：英文直通 / 拼音走 Rime | 同上 L48–L53 |
| 语音状态是字符串 `idle/speaking/thinking/done/error` | 同上 L305；多处字面量比较 L4041、L4596、L4773 |
| 面板高度已有统一来源 | 同上 L6411–L6437：220–420dp（默认 300）、抬升 0–120dp |
| 主题：`theme_mode` 覆盖系统，颜色逐处 `tone(dark, light)` | 同上 L424–L435 |
| 英文候选**长按 = 遗忘自定义词** | 同上 L3311、L3355；[EnglishCandidateProvider.kt](kotlin/EnglishCandidateProvider.kt) L76–L81 |
| 语音命令现为 `start/stop/cancel/cloud`，**无翻译/发送命令** | 同上 L63、L102–L128、L209 |

据此判断需要动的是：状态表达、手势组织、候选呈现、模式切换、尺寸/主题 token 化、生命周期清理。高度**不是**无真源，现状应沿用 `panelHeight*` 并扩展，而不是新建第二套。

## 3. 体验规格

### 3.1 双体验

- `voiceFocused`：默认落在语音页，可切到打字/剪贴板/笔画；
- `singleKeyboard`：默认落在键盘页，空格长按起语音。
- 两者共享面板高度。本版不新增第二份“体验”偏好：沿用本地模式记忆，用户停在语音页即为语音优先，停在键盘页并用空格按住说话即为单键盘路径；不会写入宿主的共享配置来覆盖本地模式。
- 初次默认语音页，后续记住用户主动选择；密码/数字/电话/日期字段只做临时适配，不覆盖该偏好。独立体验锁定开关仍是后续产品选项，不宣称本版已提供。

### 3.2 状态机与宿主协同目标

```
ImePhase        = Idle | Starting | Listening | Processing | AwaitingInsertion | Done | Cancelled | Error
ImeAvailability = Unresponsive | NeedsActivation | Preparing | Busy | Ready
```

- **S1** `Starting/Listening/Processing/AwaitingInsertion` 为 busy；busy 时默认不开放会破坏当前会话的操作。
- **S2** UI 呈现由单一 `render(snapshot)` 产生，回调里不再散改文案/可见性。
- **S3** 宿主状态带时间戳；过期即视为 `Unresponsive`，"能否开始"与 UI 共用判断。
- **S4** 目标冻结：记录 `documentId + selectionRevision`；回调时代码校验目标未变，否则拒绝而不是改投。
- **S5** 两段式启动：请求 → 下一主循环校验（目标未变、输入法可见）→ 提交；期间取消必须可中断。
- **S6** 生命周期：`onStartInput` 重置目标并**隔离过期回调**（旧 requestId、旧 `InputConnection` 身份一律丢弃）；`onFinishInput`/`onFinishInputView` 取消进行中会话与定时器。**不自动提交半成品组合**——保留现有"候选上屏/明确结束时才提交"的策略。
- **S7** busy 中用户切模式：允许"先显式取消当前会话，再切换"，不要求绝对禁用切换。

**本版实现边界**：实际 UI 阶段为 `Idle/Starting/Recording/Processing/Submitting/Done/Cancelled/Error`，通过纯投影与能力资格集中渲染；等待本入口文字结果时维持 Processing 所有权。云笔记网络提交阶段不伪装成可取消录音，可按原契约开始下一次听写。身份隔离基于 editor session epoch、requestId 与真实 backend sessionId。S3 的完整带时效可用性租约、S4 的跨宿主 documentId/selectionRevision 协议与 S5 的完整两阶段宿主启动是 iOS 对照设计目标，未在本轮更改既有 Android 后端协议来实现，不把它们列为已验证。

### 3.3 手势

| 手势 | 行为 |
|---|---|
| 空格轻点 | 输入空格；有组合先上屏组合 |
| 空格长按 | 开始语音，进入按住说话覆盖层 |
| 按住+滑动 | 覆盖层内选择"松手输入 / 取消"；翻译与发送见 §5（当前协议不支持，仅扩展提案） |
| 松手 | 以**终点坐标**判定；两识别器同时结束时延迟一帧取终点 |
| 左右滑动头部 | 切换模式，带方向动画；字母键区域不竞争此手势 |
| 删除长按 | 连发删除，沿用现有节奏 |
| 笔画输入 | 沿用现有控制器与交互 |

- **G1** 手势→动作由纯函数 `GestureResolver` 完成（手势 + 几何 + 状态 → 动作），可在 JVM 单测。
- **G2** 取消永远可用，不得被"开始中"状态吞掉。
- **G3** 触控目标优先 **48dp**（Android 惯例）；密集字母键按可达性与真机验证确定具体尺寸，**不强制每个字母横向 48dp**。

### 3.4 候选呈现

- 候选条：单行滚动，首项高亮，末尾展开入口。
- 展开面板：多行网格，覆盖键盘区但不改变面板总高度；关闭后恢复原视图。
- **长按语义保持现状**：英文候选长按 = 遗忘自定义词，必须保留。中文/拼音是否新增长按行为需单独立项，**本次不要求"长按松手上屏"**。
- 符号面板：保留现有三层切换。
- Rime：前端只消费候选列表，不解析输入串（**C1** 候选渲染用结构化比较，相同则跳过重建；**C2** 抑制我方写入触发的 `onUpdateSelection` 回环）。

### 3.5 尺寸与主题

- 尺寸沿用 `panelHeightDp/Px`、`raiseHeightDp/Px` 单一来源，仅扩展（landscape、体验差异），任何模式切换不改变总高度。
- 主题提炼 `ImePalette` 语义 token（surface/key/auxiliary/accent/warn/text/separator，各含 dark/light）；`theme_mode` 保持 light/dark/system 三态。
- 主题切换更新颜色相关内容，但保留系统 IME 根视图、编辑器对象与业务状态，不通过反复 `setInputView` 替换窗口；不宣称所有子 View 原地变色。
- 单手模式为可选后续项，不阻塞本期。

### 3.6 语音可视化与反馈

- 波形/音量来自真实电平（现有静音阈值 0.02），无样本即静音，不用合成动画。
- 触觉三档：按键 / 开始 / 错误；文案由 `render(snapshot)` 单一来源输出。

### 3.7 无障碍

- 每个可交互控件有稳定描述；候选展开/收起后更新可访问性状态；
- 状态变化发送内容变更通知（仅在无障碍焦点时）；
- 动效可关（系统动画缩放为 0 或无障碍选项）；TalkBack 下候选/预览文本可读。

## 4. 架构方向

```
OpenLessImeService          生命周期绑定 + 组装（不放具体布局逻辑）
presentation/               纯 Kotlin，可 JVM 单测，不 import android.*
  ImeStateMachine · GestureResolver · CandidateRenderState · ImePalette · ImeDimens
views/                      Android Views 组件，无业务逻辑
  VoicePanel · Waveform · CandidateBar · CandidatePanel · EnglishKeyboard
  · ClipboardPanel · ModeSwitch · PanelHost
adapter/                    外部契约适配
  NativeImeBridge · RimeCandidateSource · EnglishPredictionSource · ClipboardSource
```

约束：`views/` 不直接调用 native；`presentation/` 不依赖 Android 框架；服务不出现具体布局代码；可继续程序化构建，布局参数来自 `ImeDimens`/布局数据类。

**视觉迁移策略**：P0 允许"等价搬迁、像素不变"作为过渡手段；**这不是最终要求**，后续按本规格实现并允许有意调整。

## 5. 外部契约与可移动实现

**必须保持的外部契约**（实现位置可移动，行为不可变）：

- JNI 方法签名与语义（`nativeImeCommand`、`nativeAsrRoute`、词汇表相关方法、`nativeHasRegisteredActivityContext`）；
- 语音命令动作与 JSON 字段（`start/stop/cancel/cloud`）及宿主回调解析；
- Rime 引擎接口与候选输出格式；
- 存储格式与偏好键（`openless_ime_ui`、宿主共享偏好）；**用户词、频次、剪贴板历史升级后不丢**；
- 隐私与输入正确性保护（草稿替换、粘贴校验、学习策略、隐私判定）行为不变。

**可自由重构的**：视图实现、状态存放位置、测试方式。现有基于源码正则匹配的契约测试，可替换为**等价行为保护**（不绑定实现细节）。

**协议扩展（仅提案，不在本次 UI 重写中强制）**：翻译、发送等动作当前协议无对应命令；若产品需要，须与宿主同步定义命令与状态，UI 侧不单方面假设可用。

## 6. 分期与验收

- **P0**：状态机、手势解析、候选渲染状态、palette/dimens 骨架 + JVM 单测；服务接线；视觉等价迁移。
- **P1**：候选条与展开面板、符号面板、语音面板与真实波形、模式切页。
- **P2**：模式记忆覆盖两种使用路径、主题/尺寸语义化、无障碍完善；独立体验开关、单手模式是后续可选项。

**验收条目**（均可观察/可自动化，不依赖未核验行为）：

- AC-01 状态迁移单测覆盖：取消、丢焦点、目标变化、超时、busy 切模式。
- AC-02 手势解析单测覆盖：空格轻点/长按/滑动/松手判定、删除连发。
- AC-03 现有 Android 契约测试通过（或替换为等价行为保护）。
- AC-04 英文候选长按仍执行"遗忘自定义词"。
- AC-05 展开候选不改变面板总高度，关闭后恢复。
- AC-06 静音时波形无动画。
- AC-07 半成品组合不被自动提交；切换输入框后旧回调不生效。
- AC-08 `theme_mode` 三态切换更新呈现，普通刷新不替换系统输入视图根与编辑器对象。
- AC-09 高度 220/300/420dp 与 landscape 无裁切、无按键重叠（真机）。
- AC-10 TalkBack 可遍历按键与候选，状态变化有播报。
- AC-11 升级后用户词、频次、剪贴板历史、偏好保留。
- AC-12 宿主未启动/弱网时，可用性文案与禁用态正确。

**真机清单**（每期执行，无法自动化）：微信/QQ/短信上屏与粘贴；密码框与数字框；横屏/分屏/折叠展开；冷启动（杀进程后拉起）；宿主未启动。

## 7. 风险与限制

| 风险 | 说明 | 缓解 |
|---|---|---|
| 静态审查局限 | 本文件与 iOS 依据均未实机验证 | 真机清单 + 逐期对照验收 |
| 状态迁移漏改 | 现有大量字符串比较，易漏 | 先加兼容层，逐步替换并用单测锁定 |
| 重建丢状态 | 现有刷新会重建视图树 | 状态外置，重建只重建 View |
| Rime 组合态 | 前端重写可能与组合态不同步 | 组合态由引擎持有，前端只渲染 |
| 隐私回归 | 剪贴板面板/预览可能新增读取点 | 复用现有隐私判定，新增读取点需评审 |
| 性能未知 | 无帧率/启动时延数据 | 后续补基准，目标值需实测校准 |
| 外部参考 | README 仅证明功能声明 | 不据此推断成熟度，也不照搬实现 |

## 8. 参考

**iOS 基线（本仓库，静态阅读）**：[KeyboardViewController.swift](../ios/Keyboard/KeyboardViewController.swift)（生命周期 L232–330、布局 L425–816、空格手势 L1682–1806、状态映射 L2388–2546、插入 L2316–2379）、[KeyboardPresentation.swift](../ios/Packages/OpenLessShared/Sources/OpenLessShared/KeyboardPresentation.swift)（尺寸 L3–16、偏好独立 L18–30）、[KeyboardVoiceAvailability.swift](../ios/Packages/OpenLessShared/Sources/OpenLessShared/KeyboardVoiceAvailability.swift)（L4–20）、[SpaceVoiceOverlay.swift](../ios/PlatformShared/SpaceVoiceOverlay.swift)（L137–219）、[KeyboardStyle.swift](../ios/Keyboard/KeyboardStyle.swift)（L9–32）。iOS 同样未经本次实机验证。

**Android 现状**：[OpenLessImeService.kt](kotlin/OpenLessImeService.kt)、[StrokeInputController.kt](kotlin/StrokeInputController.kt)、[RimePinyinController.kt](kotlin/RimePinyinController.kt)、[EnglishCandidateProvider.kt](kotlin/EnglishCandidateProvider.kt)、[OpenLessNative.kt](kotlin/OpenLessNative.kt)、[ImePrivacyPolicy.kt](kotlin/ImePrivacyPolicy.kt)、[ImeDraftReplacement.kt](kotlin/ImeDraftReplacement.kt)、[OpenLessPasteVerification.kt](kotlin/OpenLessPasteVerification.kt)；契约测试见 [app/scripts](../app/scripts)（`android-*.test.mjs`）。

**外部**：HeliBoard / FlorisBoard / Fcitx5 for Android / Trime（链接见 §1）。

## 9. 官网 APK 对照与当前实现检查点（2026-11-27）

从正常 TLS 校验的官网链接取得两份安装包，仅做离线资源与类索引分析，未安装或运行竞品。包内签名验证通过，但没有取得官方独立公布的证书指纹，因此不把签名自洽等同于独立的发行身份鉴定。

| 样本 | 官网入口 | 版本 / 包名 | SHA-256 |
|---|---|---|---|
| 微信输入法 | <https://z.weixin.qq.com/android/download> → 腾讯 CDN | 4.0.2 / `com.tencent.wetype` | `f26190b30627eab5ac636c04f459618cd3211b92c5c6364d858dd9f47d3d8f29` |
| 讯飞输入法 | <https://srf.xunfei.cn/> → 官网下载入口 → voicecloud CDN | 15.0.26 / `com.iflytek.inputmethod` | `9a5ad7d957e92e506d3536a27f9aa0509e8d7d0592fea0c721759831439a63e9` |

原始包与研究证据保存在本地 `/data-win/openless-cache/agent/scratch/android-ime-commercial-research-20261127/`，不进入 Git，也不把闭源代码、布局文件、图标、词库或模型复制到本项目。

可直接观察到、而非凭类名猜测的设计：

- 微信键盘的 JSON 配置将行、按键、长按提示、动作与主题属性分开描述；横竖屏有不同的行高配置。
- 讯飞的 INI 配置将候选带与键盘带分成独立区域，并有竖屏/横屏配置和多种候选行配置。
- 这些证据支持我们采用“布局数据 + 独立渲染组件 + 语义主题”的方向；不证明竞品性能更好，也不能仅凭资源数量推断成熟度或运行期架构。
- 两者可能混合使用数据布局、XML 和代码视图；不作“微信纯代码、讯飞纯 XML”的绝对判断。未知字段不作为实现依据。

当前已接线的组件：[布局/主题模型](<kotlin/ImeKeyboardModels.kt>)、[打字/候选面板](<kotlin/ImeTypingPanel.kt>)、[语音面板](<kotlin/ImeVoicePanel.kt>)、[可访问的模式栏](<kotlin/ImeModeBar.kt>)、[可测手势仲裁](<kotlin/ImeGestureSession.kt>)。语音会话、Rime、学习与文本提交仍由现有宿主适配，未复制竞品实现。

**尚不能宣称整体验收通过**：笔画/编辑子面板的进一步解耦、横屏/大屏专项布局、主题无整树重建、双体验设置，以及完整真机交互和性能验证仍须按第 6 节逐项完成。首个可安装检查点不是“成熟输入法已全部完成”的结论。

首版检查点验证：38 项 JVM 模型/手势测试、19 项 Android 契约检查通过；arm64 debug APK 与 AndroidTest APK 构建通过，并安装到华为 LYA-AL00（Android 10 / API 29）。手机上运行了 29 项 View/手势仪器测试，全部通过；这不是实际麦克风、ASR/润色与跨应用上屏的端到端验证。启动成功，默认输入法未修改，也未自动授予麦克风/无障碍权限。启动时出现的电池优化授权由设备主人决定。此包约 667 MiB，包含调试符号，不作为最终发行体积基线。

## 10. 首版之后的本地改造（尚未推送手机）

- 新增 [通用动作键](<kotlin/ImeActionButton.kt>)、[共享候选条](<kotlin/ImeCandidateStrip.kt>)、[笔画/数字符号面板](<kotlin/ImeStrokePanel.kt>)、[编辑/剪贴板面板](<kotlin/ImeClipboardPanel.kt>)。
- [笔画控制器](<kotlin/StrokeInputController.kt>) 只保留组合、查询、学习、提交和组件装配，不再包含逐键布局与绘制代码；文字输入引擎和持久化格式未改。
- 打字与笔画使用同一候选组件，复用相同标签的控件但始终更新回调；旧候选、关闭的展开项和阻断态不能误提交。
- 编辑/剪贴板提供明确的收藏、词典、删除入口，不再使用旧的“滑动取消事件也可能提交删除”的行手势；粘贴成功后才更新最近历史。
- 保留笔画分词、繁简、1–9 上滑数字、空格长按语音、数字/符号页的退格/回车/语音；翻页按最新数据派发字符，避免只更新标签却输入旧值。
- 普通的分词/候选、编辑选择、历史分类切换局部渲染，不替换整个 IME 根视图；草稿开关纳入根布局标识，且只由实际重建确认新标识。

这一批组件的独立 Kotlin 编译和 38 项 JVM 回归已通过，19 项 Android 契约检查继续通过。另新增 25 项剩余面板仪器测试；本地 Robolectric 适配只改测试框架，不改断言、使用空 Application 隔离 JNI，也不操作手机。首次完整 View 测试当时仍在准备依赖；后续结果见第 11 节。手机仍保留首版归档包，不把这一批本地源码当成已安装版本。

## 11. 本地行为验证与旧实现清理

后续 Robolectric 运行保持 SDK 29、空 Application 隔离 JNI，测试体与 Android 仪器版本相同。首轮 53 项发现两处失败：按键在 UP 时过早清掉“曾越界”标记，导致移出又移回后误点击；以及测试错误地从候选条本身而非其独立展开层查找节点。修正生产代码与真实视图层级夹具后，最新 **54 项全部通过（25 剩余面板 + 19 打字面板 + 10 语音面板，0 skipped / 0 failure / 0 error）**，Android 主模块编译也通过。

- 原 Service 中未再使用的 `keyboardKey`、旧候选浮层、画布键帽、旧滑动容器等实现已删除；Node 检查改为新组件边界与测试装配守卫，不再对死代码片段声称“手势通过”。
- 新增 [共享头部](<kotlin/ImeKeyboardHeader.kt>)，设置入口、后端指示动画与头部导航交互由 View 组件拥有；导航手势限定头部，不与字母键拖动竞争。
- [语音 UI 投影](<kotlin/ImeVoiceUiState.kt>) 将阶段、模式、电平与可取消/可启动能力集中表达，且不回写业务 flags。SDK-free 模型回归累计 **64 项通过**（含投影、所有权匹配与帧策略/资格原型）。
- 悬浮球仅镜像其他入口的录音帧，不因该帧自动提升麦克风前台服务或在提升失败时取消其他入口的录音；主动全局停止/取消行为保留。启动意图 guard 的 **6 项单元测试通过**，不把它宣称为强会话绑定。

**R2 发现的会话来源缺口**：Core 胶囊负载有 sessionId，但当时 Android 桥没有透传，仅有“自己发起了请求”的标记不足以判断每一帧来源。后续兼容接线和验证见第 12 节。Core 的重复 stop 在处理中会返回 Busy，不能通过重复发 stop 来补取别的入口已经结束的结果。未收到本入口结果时不得宣称已上屏或擅自补写。

以上为本地源码和模拟环境结果；本节改动尚未安装到手机，首版真机的 29 项结果不能冒充最新源码的端到端验证。

## 12. 会话元数据与原生接线验证

- Core 胶囊出口现把原有 `payload.session_id` 传到新增的 Android UI 回调；旧三参数回调保留。新增 descriptor 为 `(Ljava/lang/String;Ljava/lang/String;FLjava/lang/String;)V`，编译后 Java 静态方法与重编原生库均已核对。现有 IME start/stop/cancel 命令和 ASR 协议未改。
- [会话帧门禁](<kotlin/ImeScopedFrameGate.kt>) 按当前 request/epoch 与 started 确认的真实 sessionId 工作。早帧只做有界缓存并仅补投匹配 id，其他 id、无 id、已退休 id 不改自有录音状态或电平。结束、取消、旧 owner、新 owner 与重复 started 均有保护。
- started 到达时若缓冲帧已进入整理或终态，不再补发排队的模式/stop 命令；停采集后仍保留本入口结果所有权，避免过早开放新录音而丢掉旧结果。
- 新增桥接兼容测试与 [Service 实际接线测试](<kotlin/androidTest/ImeSessionScopeInstrumentedTest.kt>)，后者只附隔离 Context、不运行 onCreate、不发 Native/ASR 命令、不接触编辑器，只调用实际帧处理路径。

验证：**97 项 SDK-free 测试、65 项本地 View/Service/桥接兼容测试、23 项 Android 架构/契约检查通过**。另完成完整 Android Rust + Kotlin APK 构建，未用旧 `.so` 跳过原生验证。编译/构建仍有既有弃用与 Rust 警告，不能称零警告。

本地新调试包经签名与 zip 内容核验，约 667 MiB。增量打包曾留下约一份旧原生库大小的无效空间，已将该未发布产物保存在 scratch，再重新生成紧凑包；未清除共享构建缓存，测试库也未混入主 APK。新归档 SHA-256 为 `c00ab27c3fa0e58aa5089bcfcec9045ef156b7800b895079b1f0dae58e00f80a`，手机仍保留最初的 `1530c30d` 体验包，未覆盖。

限制：JNI 元数据接线已编译和 ABI 检查，本地测试不等同于实际麦克风/ASR/润色/跨应用上屏端到端测试。其他入口结束 IME 发起的录音时，现有后端未必发送本入口的文字回调，UI 不自动猜测或补写结果；这不是修改 provider、存储或 Core stop 语义。后续布局与手机检查结果见第 13 节。

## 13. 第一版重写交付与手机验证（2026-11-28）

本版采用稳定的 [输入视图根](<kotlin/ImeSurfaceHost.kt>)，普通模式/主题/草稿刷新只替换展示内容，不重复 `setInputView`；[尺寸策略](<kotlin/ImeSurfaceLayout.kt>) 使长草稿在独立区域滚动，受限空间先压缩抬升区再压键盘，不从每个 EXACT 测量结果反复扣减边距。实际 Service 的主题刷新、草稿重挂文本/选区保持和重复测量不缩小均有测试。

- [草稿栏](<kotlin/ImeDraftBar.kt>) 独立于业务：复用同一 EditText，Service 保留 DraftConnection、提交/替换/丢弃语义；提交中按钮禁用，旧聚焦请求不能抢新栏。
- [连发策略](<kotlin/ImeInteractionPolicy.kt>) 统一沿用旧偏好的 30–200ms 范围与 60ms 默认值。打字、语音 footer、笔画和编辑按键接入反馈 hook，DOWN/UP 不双重反馈，同值策略不取消触摸，策略变化取消旧 timer。
- [设置预览](<kotlin/ImeKeyboardPreview.kt>) 使用真实新版打字组件，不再是三行假键；预览屏蔽输入事件和虚假的可访问性动作，不请求语音或输入引擎。
- [字段适配](<kotlin/ImeEditorFieldPolicy.kt>) 为数字/电话/日期字段临时选数字页，不覆盖用户本地模式偏好；缓存页面也会刷新当前层。
- [选区策略](<kotlin/ImeSelectionPolicy.kt>) 使用绝对 UTF-16 偏移，处理 extracted text 的 startOffset，未知选区不猜 0；range 与退格按 codepoint 保护 surrogate pair。只承诺 codepoint，不宣称完整 ZWJ/grapheme 编辑。
- 补齐父面板禁用的 clear/candidate 传播、稀疏符号页变满后的槽可用性，以及语音 footer 动作后重新校验再续排 timer。

**最终自动化证据**：123 项纯 JVM 测试、98 项本地 SDK 29 View/Service 测试、24 项 Android 架构/契约检查通过。主 APK、AndroidTest APK 构建通过；R3 已完整重编匹配的 Android Rust/JNI 库，后续仅 Kotlin 改动复用该库。仍有既有弃用/Rust 警告，不宣称零警告或性能基准达标。

经用户明确确认后，在华为 LYA-AL00 / Android 10 API 29 上执行 `adb install -r -t` 安装主包及测试包，均 Success，未卸载或清数据。安装前系统未列出此包，故不能把本次安装声称为已有用户词库的实测迁移；存储格式与相关存储实现未修改。手机上指定 10 个 UI/宿主测试类执行 **OK (98 tests)**，用时 4.145s，退出成功。没有运行会初始化真实 Rime/ASR 或改真实用户偏好的其他测试类。

已安装包 SHA-256：`ae865bfe82fe1528cac242c57f012c9adf94864303fa9e5a991d6c69644e2ce9`，大小 699,453,401 bytes（约 667 MiB，含调试符号），签名 v2 校验通过。原体验包及各阶段产物仍在机械盘 scratch；未清除构建缓存。

启动检查：应用进程存在，启动进入系统“忽略电池优化”请求，是否允许由设备主人决定，未代点。默认输入法仍是华为输入法，未自动启用 OpenLess、未授予麦克风或无障碍权限。实际日常输入、TalkBack 朗读、麦克风/ASR/润色及跨应用上屏仍需设备主人配置权限与服务后体验，不把上述 98 项 fake-callback/View 测试当成这些场景的端到端结果。

**搜狗样本范围**：本次再次尝试官网首页、手机版落地页与官方旧手机版入口，正常 TLS 获取官方页面并在 scratch 留下 URL/headers/hash，但未取得可信 APK 直链；未转向第三方镜像或绕过反爬。实际 APK 离线研究样本为微信与讯飞两份，不宣称已分析搜狗 APK。
