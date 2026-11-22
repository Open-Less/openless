# Issue #1238：手改学词建议卡片靠近输入目标的调研

> 调研日期：2026-10-24。仅静态阅读源码、GitHub 元数据和官方 API 文档；没有运行应用、测试、编译或打包。
>
> 工作分支 `HKLHaoBin/issue-1238` 的 HEAD 为 `1462581a0114ce43ec3822ccaaf20ad00c48c3a5`。主要研究基线固定为上游 `beta` 的 `38fbe1c082e782e566e13769e4675a183ccc812a`；下列源码引用均指向该提交，除非明确写为旧 HEAD。

## 1. 结论摘要

**可以实现，但不是把右下角的两个偏移量改成光标坐标那么简单。** 现有链路保存了插入目标身份、文本范围和建议 ID，却没有把“属于本次插入的屏幕位置”传到卡片展示端。[目标接口][target-port]、[观察接口][observation-port]、[建议结构][suggestion-types]

推荐：**默认在本次输入目标附近显示非模态确认卡；优先可靠的文本插入光标矩形，失败时退到焦点输入框，再退到原目标屏幕的角落。** 卡片不抢焦点、不覆盖正在编辑的行，不持续追逐鼠标或每次按键；保留逐条确认、过期不入库。位置偏好“输入位置附近 / 屏幕角落”可以作为可选设置，但不能替代正确的数据关联与降级链路。

这是交互改进，不是已证实的识别或学习逻辑故障。原卡片源码注释明确说明右下角是为了避免挡住编辑行；改到输入附近时仍须解决遮挡问题，不能只提高可见性。[原设计与前端行为][card-ui]

## 2. 需求、历史与研究基线

- [Issue #1238][issue] 报告 Windows 11、`2.0.1+build.20261016`、粘贴上屏、开启手改学词；微信中观察和卡片正常，问题是卡片离输入位置太远，短暂出现时容易错过。第一选择是靠近本次插入的光标/焦点元素，另一选择是提供位置偏好。
- 调研时 issue 仍 OPEN，只有维护者“满足你。”的评论。查询 issue timeline 和按 `1238` 搜索 open/merged PR，未发现关联实现；这是本次查询结果，不是对所有分支的穷尽证明。[Issue][issue]
- Windows/Android 学词来自先前扩展工作。#1127 的关闭说明明确指出经集合 PR #1126 合入；#1128 本身 CLOSED 且无 merge commit，不能据此断言 Windows 学词未落地。[#1127][history-issue]、[#1126][history-release]、[#1128][history-pr]
- 当前分支明显早于 issue 报告版本。`git rev-list --left-right --count HEAD...38fbe1c0` 为 `2 / 132`，即当前分支独有 2 个提交、缺少上游 132 个提交。这里只 fetch 参考，未切换分支或合并。实施前应先有意识地处理基线差异，不能直接把旧 HEAD 当最新版本。
- 旧 HEAD 前端是整卡固定 10 秒、建议变化重置 timer、到期全量 dismiss；固定上游已改成每条建议的绝对 deadline 和按 ID 过期。不要从旧文件重新实现旧超时机制。[旧前端][old-card-ui]、[上游前端][card-ui]

## 3. 已确认的源码现状

### 3.1 固定角落的直接原因

`tauri_coordinator_host.rs:1568-1591` 的 `position_vocab_card(width, height, edge_margin)` 只读取**卡片所复用窗口的 `current_monitor()`**，将 monitor 的 physical size/position 除以 scale，然后用 `LogicalPosition` 设置右下角：横向减卡片宽度及 24 的边距，纵向减卡片高度及固定 80。既不接收光标/焦点元素，也不使用 monitor work area。[定位函数][position]

当前不是“光标定位失败才回退”，而是**完全没有光标定位分支**。`current_monitor()` 也不能证明这就是本次输入目标所在屏幕。混合 DPI 与任务栏避让需随改动覆盖，但本次没有实机证据证明某个具体 DPI 组合已发生偏移。[定位函数][position]

### 3.2 插入目标捕获不等于位置捕获

Core 的 `CapturedTextTarget` 只组合插入器与 host-context reader；`HostContextCapture` 只有应用标识和光标附近的文本上下文。`capture_target()` 注释要求同步路径只冻结身份，不应为定位加入慢 UIA 查询。[目标接口][target-port]、[插入与观察接口][observation-port]

Tauri `capture_target()` 捕获 `SelectionInsertionTarget` 和 Windows IME target，留在插入器中，没有 caret rectangle 或屏幕 anchor。随后 `TauriEditObservationAdapter::arm()` 只传插入文本、时长和 `EditPair` callback，未把捕获的插入目标交给 watcher。[目标捕获][adapter-target]、[观察适配器][adapters]

所以 HWND、process/thread ID、UIA identity 或 UTF-16 offset 都不能当屏幕坐标。“插入器持有目标”没有解决“建议卡知道原目标在哪里”。

### 3.3 Windows 有文本 caret 能力，没有卡片几何链

`host_document/windows.rs:529-613` 的观察器绑定 focused UIA element，拒绝密码框和不允许的进程，检查前台 HWND 与 focused element 是否仍一致，监听 Text/Value 变化，静默约 700ms 后读文本并分析插入文本范围。事件 handler 只标 dirty，观察和清理在 worker 中完成。[Windows 观察器][windows-observer]

同文件 `626-699` 的另一条光标上下文读取路径尝试 `TextPattern2::GetCaretRange` 并检查 `isActive`；兼容路径是单个 collapsed selection。返回的是 **UIA 文本 range**，不是光标屏幕矩形，可参考但不能直接传给 `set_position`。[caret 查找][windows-caret]

窗口关闭或切换控件已有观察保护，但 worker 检查、发布建议到主线程显示仍有间隔。新定位须处理自身快照失效，不能把“观察时有效”当成“显示时永远有效”。[观察器][windows-observer]、[展示末端检查][coordinator]

### 3.4 建议拥有内容和时效，没有来源位置

`PendingCorrection` 只有 `id`、`expires_at_ms`、`pattern`、`replacement`，最多 5 条。学习设置默认观察 60 秒、展示 10 秒，规范化范围分别为 10–60 秒和 5–60 秒；不是硬编码“所有版本都展示几十秒”。[建议与设置][suggestion-types]

Core sink 有 observation generation，生成建议再次核对 generation，分配 UUID、计算绝对 deadline、发布 `VocabularySuggestionsChanged`。但事件 session 参数为 `None`，payload 没有 observation/session/placement 关联键。`EditObservationSink::publish` 只返回 bool，也不把新 suggestion ID 交回 host。[生成建议][queue]、[arm/disarm][arm]

**不能只增加 `suggestion_id -> anchor` 表就认为链路接通。** 还须让 host 知道建议属于哪个观察代次，或从 Core 获得创建结果；现有 bool callback 和纯建议列表没有这个关联。

### 3.5 卡片共用录音胶囊窗口

`coordinator.rs:66-187` 将卡片宽度设为 320 logical points，高度随条数变化。显示时缩小 capsule 窗口、开启点击命中、定位、失效布局缓存后显示；主线程末端检查听写 Idle 等活动会话状态，避免覆盖录音胶囊。关闭时隐藏并恢复 capsule hit-test。[展示生命周期][coordinator]

事件桥重新读取当前 pending corrections，不直接使用可能迟到的事件 snapshot，以免已关闭的卡片复活。位置元数据也应遵循“读当前状态并校验代次”。[事件桥][event-bridge]

上游前端逐条 accept/reject、按 deadline 到期，点击先本地隐藏，提交失败恢复；按钮 `onMouseDown.preventDefault()` 用于减少点击夺焦点。Windows 原生显示优先 `SW_SHOWNOACTIVATE`/`SWP_NOACTIVATE`，失败会回退普通 `window.show()`；不能仅凭静态代码宣称所有路径都绝不激活窗口。[前端][card-ui]、[原生显示][show-window]

### 3.6 可复用能力与平台边界

- `capsule_mirrors.rs:27-99` 的 `MonitorRect` 已统一 virtual-desktop physical coordinates、工作区、scale 和 monitor identity；`256-275` 有 logical size 转目标屏 physical size、work-area clamp 的思路。应复用类型、转换与边界处理，**不是直接使用底部居中的 capsule placement 结果**。[多屏类型][monitors]、[placement][monitor-placement]
- macOS 的 cfg-gated helper 已有 focused-input/caret anchor 和 AX 范围读取，可参考平台接口；其坐标体系不能直接复制到 Windows。[macOS helper][macos-anchor]
- 本次聚焦 Windows 桌面建议卡，不要求新增 Android 悬浮窗功能，不改变识别模型、学词规则、批量接受策略或所有平台普通胶囊的位置。

## 4. 官方 API 核查：必须保留失败路径

### 4.1 UIA range 不保证能得到矩形

Microsoft `GetCaretRange` 返回零长度 range，`isActive` 表示文本控件是否拥有键盘焦点；为 false 时不能当成当前系统 caret。[GetCaretRange][ms-caret]

`GetBoundingRectangles` 明确指出：degenerate/empty range、完全屏外、滚出可视区或被遮挡时会返回空数组。因此 **`GetCaretRange -> GetBoundingRectangles` 不能写成必定成功的通用方案**。可以尝试并验证结果，空结果必须正常降级。[GetBoundingRectangles][ms-rectangles]

若用相邻字符的非空 range 估算位置，须明确只是近似，并验证文末、空行、多行、RTL、选区。本次未验证此算法，不列为首版必做。焦点元素矩形是粗定位，不能把整个编辑区中心冒充 caret。

### 4.2 Win32 caret 不能直接上屏

`GUITHREADINFO.rcCaret` 是相对 `hwndCaret` 的 client coordinates；`GetGUIThreadInfo` 说明其为所属窗口意义上的 logical coordinates，不会自动转换成调用线程的 DPI 模式。窗口失去激活时 handle 可能无效；文本方向和 padding 也影响实际插入点。[GUITHREADINFO][ms-gui-struct]、[GetGUIThreadInfo][ms-gui]

可用此路径补充原生 edit control，但必须校验原目标、handle、矩形，显式完成客户区到屏幕及 DPI 语义转换，不能盲目乘一个 scale，也不能把当前线程 `GetCaretPos` 当跨进程目标 caret。

### 4.3 物理屏幕坐标与工作区

Microsoft 明确 UIA 不使用 logical coordinates，相关 bounding rectangle 是 physical。仓库锁定的 Tauri 2.12.4 中，Monitor position/size/work_area 也是 physical 类型。[UIA 缩放][ms-scaling]、[Tauri Monitor][tauri-monitor]

推荐 Windows host 统一输出**带符号的 virtual-desktop physical rectangle**，用 anchor 所在屏幕的 scale 转换卡片 logical size/间隙，按该屏 work area 翻转、clamp，再设置 physical position。不得对整个多屏桌面坐标统一除以一个屏幕 scale；负坐标合法。Tauri 已有 `monitor_from_point` 及对应 position/size 类型接口。[Tauri 选屏][tauri-screen]、[定位接口][tauri-position]

### 4.4 线程、权限与隐私

Microsoft 建议通用 UIA 客户端在无窗口的独立 MTA 线程调用 UIA，同线程注册/移除事件；跨线程 marshal 的元素受原 apartment 生命周期影响。[UIA threading][ms-threading]

不要把 UIA COM object/text range 存进前端 payload 或全局位置缓存，不要在 Tauri 主线程同步读宿主几何。只传纯数据矩形、来源、时间与关联键，复用密码框/进程门禁。几何读取失败不应阻断插入或学词确认；无需注入目标进程、OCR、额外常驻全局监听或上传宿主文本。

## 5. 推荐接入点与实现边界

以下只是后续建议，未修改业务代码。

### 5.1 先贯通数据关联，再移动窗口

1. **冻结身份**：保留 session 开始时的目标捕获，不在该同步接口增加慢 UIA round-trip。
2. **捕获位置**：插入完成、准备观察时在 worker 对照原目标捕获可选 caret/input rectangle。检测到有效手改且仍是同一目标时可更新一次，避免 60 秒内窗口移动后一直用旧坐标。快照包括目标身份、物理矩形、来源、时间和 observation 关联键。
3. **贯通关联键**：扩展 adapter/sink/呈现元数据，让建议带可移植的 observation/session 关联键；平台几何可留 Tauri 短生命周期 registry。另一方式是 publish 返回结构化创建结果，再按 suggestion ID 建表。应结合其他 host 兼容性选最小改动，不能用全局“最近一次位置”代替关联。
4. **显示前复核**：读取当前建议与同源位置，主线程窗口操作前检查 presentation generation、deadline、活动会话；旧建议/矩形/取消 session 不能覆盖新卡片。
5. **清理**：accept/reject、过期、新 session、关闭学习、目标失效时清理对应位置。几何不写词典或云同步数据。

主要接入点：`ports.rs`、`api.rs`、必要的 shared/event metadata、`core_adapters.rs`、`host_document/windows.rs`、`coordinator.rs`、`tauri_coordinator_host.rs`。确定关联方式后再收敛实际文件，不预设全部都必须改。

### 5.2 定位与降级

- **优先**：同源且有效的 caret rectangle，下方留间隙，空间不足翻上方，必要时侧移。避开编辑行与已知输入法候选区域；无法可靠判定候选区域时不宣称完全无重叠。
- **其次**：同源焦点输入元素的可见矩形。过大编辑区只作粗定位，必要时退角落，避免覆盖主编辑区域。
- **再退**：原目标窗口所在屏幕的 work-area 角落。原目标关闭/失焦时不去读另一应用文本为旧建议“跟随”，可退角落或关闭该次呈现。
- **最终**：原屏断开或窗口位置不可用时，按定义好的屏幕回退策略选可用屏并 clamp。鼠标只作选屏兜底，不能标为 caret 定位成功。
- 每次显示/更新一批建议决定位置，不因每次按键重新移动卡片，避免用户追着确认按钮跑；必要更新也受同一 presentation generation 约束。

### 5.3 配置取舍

首版可先完成自动靠近输入目标与可靠角落降级。如同时加偏好，建议“输入位置附近（默认）/屏幕角落”，明确默认值、旧配置迁移及说明。普通胶囊“光标所在屏幕”只决定 monitor，不应复用成卡片 placement preference；文本插入光标与鼠标指针不能混为一谈。

偏好若进入全局 Preferences，还须覆盖持久化、前后端类型、设置页和国际化；不能为绕开这些成本就把原角落直接换成鼠标位置。

## 6. 非模态交互契约

- **需求与风险**：用户仍在另一应用编辑，确认一条纠错是否入词典；未经确认不写入，超时只是放弃本次建议，不加入拒绝名单。[现有语义][suggestion-types]
- **组件**：复用有操作的非模态确认卡，语义接近贴对象的简短 Popconfirm/建议气泡，不是纯 Tooltip 或“保存成功”Toast。无需 Modal、遮罩、焦点陷阱，不能因靠近光标而阻断输入。
- **焦点**：自动出现不激活窗口、不移动输入焦点；点击记住/忽略后可继续在原输入框打字。原生 no-activate fallback 需单独实测。[显示路径][show-window]
- **动作**：“要记住这个改法吗”及 `原词 → 新词`，按钮明确为“记住 / 忽略”；保持逐条接受，不新增“全部接受”。提交失败恢复该项并给予可理解反馈，不把点击当保存成功。
- **生命周期**：继续按每条绝对 deadline 过期，移动/追加不延长旧条目。新录音释放共享窗口给 capsule。外侧点击不阻止编辑，不全局劫持 Escape/Enter，以免吞掉宿主或输入法按键。[前端][card-ui]、[共享窗口][coordinator]
- **可访问性**：不假定原生不激活卡片能自然参与其他应用 Tab 顺序；显式键盘入口/替代确认路径需设计与实测，不能用强制夺焦点换取键盘可达。保留按钮可访问名称，长词可查看全文。
- **时长**：最小范围是位置。延长展示、悬停暂停、待处理中心属于额外产品决策，须与 Core 绝对 deadline 一致；不能只停前端 timer，使按钮仍显示但后台已拒绝接受。

## 7. 实施后的验收清单

以下均**待 GitHub Actions 或桌面实机验证**，不是本次运行结果。

- 微信、原生编辑框、浏览器 textarea/contenteditable、Electron：插入后手改，建议靠近原目标；不支持 caret provider 时退输入框/角落，不阻断学词。
- 100%/125%/150%/200%、混合 DPI、多屏左/上负坐标、任务栏移边：完整落在目标屏 work area、大小正确、边缘翻转。
- 空框、文末/空行、多行、滚动、非折叠选区、RTL：无 `(0,0)` 假 caret，不把整控件当精确插入点，按约定回退。
- 改词后切应用/控件、关窗口、拖窗口、断显示器：不跟到无关应用，不使用失效 anchor、不掉屏外。
- 卡片出现/点击后继续输入：前台窗口与文本焦点不被意外改变；覆盖 no-activate 正常与 fallback 路径，候选框/编辑行不被严重遮挡。
- 新听写：旧显示任务不覆盖新 capsule，卡片结束恢复尺寸、布局缓存与鼠标穿透。
- 连续纠错、多条先后到期、接受/忽略与新事件交错：位置匹配当前来源，旧 timer/observer 不清掉或移动新卡片。
- 密码框、禁用进程、关闭学习、权限不足、UIA 超时：隐私门禁保持、失败快速或降级，不阻塞 UI 线程、不额外记录文本。
- 如加偏好：旧配置加载、切换生效时机、重启持久化、普通胶囊设置无回归。
- 自动化重点：纯几何 flip/clamp、来源 ID 与清理、事件乱序、逐条 deadline 和失败恢复。不在本地编译或跑测试。
- 后续代码推送后，按实际分支与准确 SHA 触发 GitHub Actions CI、桌面及 Android 产物构建；先提供桌面产物。本次无代码推送，未触发构建。

## 8. 完成状态与证据边界

已完成：issue/历史核查、固定上游源码链路、Windows/Tauri 官方定位及线程契约、实现接入点、交互与验收建议。

未执行：应用复现、具体微信版本的 UIA provider 探测、混合 DPI 实机验证、功能修改、配置实现、提交/推送/PR/评论、CI/产物构建。provider 是否提供精确 caret rectangle 仍须实测，不承诺所有宿主都能精确跟随。

[issue]: https://github.com/Open-Less/openless/issues/1238
[history-issue]: https://github.com/Open-Less/openless/issues/1127#issuecomment-5912873585
[history-release]: https://github.com/Open-Less/openless/pull/1126
[history-pr]: https://github.com/Open-Less/openless/pull/1128
[position]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/src-tauri/src/tauri_coordinator_host.rs#L1568-L1591
[target-port]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/crates/openless-core/src/ports.rs#L34-L55
[observation-port]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/crates/openless-core/src/ports.rs#L794-L860
[suggestion-types]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/crates/openless-core/src/shared_types.rs#L257-L347
[adapters]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/src-tauri/src/core_adapters.rs#L3834-L3866
[adapter-target]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/src-tauri/src/core_adapters.rs#L4032-L4046
[windows-observer]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/src-tauri/src/host_document/windows.rs#L529-L613
[windows-caret]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/src-tauri/src/host_document/windows.rs#L626-L699
[queue]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/crates/openless-core/src/api.rs#L2669-L2765
[arm]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/crates/openless-core/src/api.rs#L7304-L7368
[coordinator]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/src-tauri/src/coordinator.rs#L66-L187
[event-bridge]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/src-tauri/src/tauri_events.rs#L467-L476
[card-ui]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/src/components/VocabSuggestionCard.tsx#L1-L199
[old-card-ui]: https://github.com/Open-Less/openless/blob/1462581a0114ce43ec3822ccaaf20ad00c48c3a5/openless-all/app/src/components/VocabSuggestionCard.tsx#L23-L47
[show-window]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/src-tauri/src/tauri_coordinator_host.rs#L511-L614
[monitors]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/src-tauri/src/capsule_mirrors.rs#L27-L99
[monitor-placement]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/src-tauri/src/capsule_mirrors.rs#L256-L275
[macos-anchor]: https://github.com/Open-Less/openless/blob/38fbe1c082e782e566e13769e4675a183ccc812a/openless-all/app/src-tauri/src/lib.rs#L2438-L2621
[ms-caret]: https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomationtextpattern2-getcaretrange
[ms-rectangles]: https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomationtextrange-getboundingrectangles
[ms-gui-struct]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-guithreadinfo
[ms-gui]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getguithreadinfo
[ms-scaling]: https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-screenscaling
[ms-threading]: https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-threading
[tauri-monitor]: https://github.com/tauri-apps/tauri/blob/tauri-v2.12.4/crates/tauri/src/window/mod.rs#L55-L102
[tauri-screen]: https://github.com/tauri-apps/tauri/blob/tauri-v2.12.4/crates/tauri/src/window/mod.rs#L1695-L1702
[tauri-position]: https://github.com/tauri-apps/tauri/blob/tauri-v2.12.4/crates/tauri/src/window/mod.rs#L1923-L1968

## 9. 功能实施（2026-10-24）

本节更新实施状态；上面的“未修改功能代码”描述的是调研阶段。

- 实施分支：`feat/issue-1238-vocab-card-anchor`，直接基于上游 `38fbe1c0`，不包含本地 beta 的独有提交。
- Core 建议携带可选 observation ID，sink 暴露冻结的 session/observation 身份；手动入队不带来源，原有确认、去重和绝对到期语义保持不变。
- Tauri managed state 只保留有界、短生命周期的 session 目标和纯数据位置；待交接目标不因 arm 前的 disarm 被清掉，旧代不能覆盖新代位置。
- Windows 在现有 MTA 观察器中验证原始目标，尝试 UIA caret 矩形，空结果退焦点输入元素；几何失败不阻断正常学词。显示前再次轻量验证目标/窗口，失效时退源屏角落，不改抓新焦点。
- 卡片使用目标屏物理坐标、DPI 与工作区，空间不足向上翻转；移动/缩放使用 `SWP_NOACTIVATE`。卡片点击处理 native `WM_MOUSEACTIVATE`，严格显示路径失败不回退 activating show，不改变普通胶囊策略。
- 共享窗口的 card mode 停止常规 layout watcher；show/hide 按 presentation epoch 丢弃旧任务，末端重读建议和活动会话。
- 新增 std-only 定位/registry/epoch 回归，并由独立 `backend-tests` 入口在 Windows Actions 实际执行。当前尚未运行这些测试或构建；待 Bugbot 审查后提交推送，使用该分支准确 SHA 触发 CI、桌面及 Android 产物。
- 按追加需求，在“实验与拓展 → 手改学词／修改学习”现有子页增加 Windows 建议卡位置选择，不在普通设置新增入口。可选“输入位置附近”（默认）和“固定右下角”，后者跳过近端 anchor，仍使用原输入目标所在屏幕的工作区。旧配置缺少字段时默认靠近输入；恢复默认参数同时恢复此选项。更改后按现有规则结束观察、清除待确认建议，下次听写生效。
- 不新增 Android UI，不改变其他平台的卡片定位。微信实际 provider 能力、候选框重叠和真实焦点行为仍是安装桌面包后的实机验收项目，不以纯逻辑测试代替。

审查记录：Review Bugbot 首轮两个 MEDIUM（重复 hide 清理、长听写 target 过期）已修复并补回归，复审明确返回 `No bugs found`；追加的位置选项审查也返回 `No bugs found`。首轮远程 CI/Android 完整日志揭示测试 fixture 漏初始化 epoch、Windows 专用目标校验未按平台编译两处问题，已补齐初始化并限定 Windows 编译；将复审后为新提交重新触发三项工作流。远程通过不替代上述实机验收。
