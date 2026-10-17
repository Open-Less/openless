# PR #1197 UI 只读审查

## 结论与审查边界

原始 HEAD `0947c05a5b91` 存在能解释用户反馈的具体实现问题：普通页面的 exit 被删掉，Settings 只淡出内层却保留不透明外层，新的全窗 Settings 覆盖原生拖动条，实验 ASR 的每秒轮询会触发旧 SelectLite 抢回选中项，ShortcutRecorder 直接替换内容造成高度跳变。主 Agent 当前 worktree 已处理这些主路径的大部分；本报告末尾另列追加检查发现的裸 Modal 与交付前复核状态，不能把原始分支问题当成尚未修复的当前状态。

审查基线为 `0454dc56..0947c05a`，覆盖 `openless-all/app/src` 与 `e2e` 的变更清单、交互实现和新增测试；着重检查 UI 提交 `8c32db9e`、实验 ASR 提交 `16a73b9d` 及后续测试修正。行号未注明“当前 worktree”的均指固定 HEAD，可用 `git show 0947c05a:<path>` 核实。仓库一开始就有主 Agent 的未提交修复，因此动态验证使用 `git archive 0947c05a` 导出的临时副本，没有改仓库源码、设置、凭据或安装依赖。

验证使用已安装 Playwright 的 headless Chromium/WebKit、1300×835、zh-CN、macOS UA；部分场景显式将临时浏览器的界面大小设为 100%。Vite 仅按需转换源文件，没有完整构建或运行全套 e2e；浏览器请求限制在本机临时服务器，`isTauri=false`，没有实际录音、麦克风捕获、OAuth、云同步或收费 API。新增 PNG 是静态视觉参考，不能证明动态过渡和原生标题栏行为。

## 可核实问题与最小修复边界

### 1. P2：普通页退出没有淡出，直接拆掉旧节点

- **位置**：`openless-all/app/src/lib/motion.ts:246-253`；调用端 `src/components/FloatingShell.tsx:166,533-534`。
- **触发**：鼠标从 History 切回 Overview，或切换其他主导航页。
- **原因**：`usePageTransition` 直接 `return requested`，只调用 `useContentMotion`；外层又是 `key={displayTab}`，旧页在本次 React commit 立即卸载，不存在旧内容的 exit 阶段。
- **引入提交**：`8c32db9e` 明确删去基线的 displayed 状态和 PAGE_EXIT_MS 流程。
- **已验证**：两个浏览器从 History→Overview 均只创建新页的 `[opacity:0, translateY(6px)]→[opacity:1, none]`、240ms 动画，没有旧页 exit；没有 pageerror。
- **最小建议**：在共享 hook 保留 outgoing displayed，先对旧页做短 opacity exit，再切最新请求页并 opacity enter；用代次/取消处理快速 A→B→A，保留 reduced-motion 分支，不需要引入路由库或重写全部页面。当前 worktree 已恢复这种 hook，但本审查未对新实现重新跑浏览器。

### 2. P2：Settings“关闭”看似没淡出，是外层不透明遮挡

- **位置**：`src/components/SettingsModal.tsx:94-95,332-344`。
- **触发**：Settings 已打开，用返回或右上角关闭退出。
- **原因**：`overlayRef` 的 `useOverlayMotion(..., enabled=false)` 被关闭，但 workspace 被改成全窗不透明 `background:var(--ol-surface)`；只有 inner surface 在淡出。底下主页面一直被外层实色挡住，180ms 到时整层卸载才突然露出。
- **引入提交**：`8c32db9e`。
- **已验证**：Chromium/WebKit 中 inner opacity 已降到约 0.001，outer 仍是 `opacity:1`、`rgb(28,28,31)`；随后 outer/inner 同时消失。这是合成结果问题，不是没有调用 exit，也不是需延长退出时间。
- **最小建议**：只对整个不透明 workspace 做 opacity enter/exit，消除内外层重复动画；继续复用 `useExitMount` 维持生命周期。当前 worktree 已改为整体淡出。

### 3. P2：Settings 顶部覆盖拖动条；重复关闭入口确实存在，但自绘红绿灯归因不对

- **位置**：`src/components/SettingsModal.tsx:332-344,411-428,601-612`；`src/components/WindowChrome.tsx:67-77`。
- **触发**：macOS 主窗口打开 Settings 后拖顶部空白；观察 sidebar“返回”和右上角“关闭”。
- **原因**：全窗 Settings 的 z-index 50 覆盖同为 50、但先绘制的 WindowChrome drag region；sidebar 顶部 padding 仅留出视觉位置，并未赋予拖动能力。新增 sidebar 返回与原右上角关闭都调用 `closeSettings`，属于重复出口。
- **引入提交**：覆盖与重复入口由 `8c32db9e` 的全窗设置/新增返回引入；WindowChrome 旧的 z-index 本身早已存在。
- **已验证**：两个浏览器 `elementFromPoint(150,12)` 在打开前命中 drag region，打开后命中 Settings `ASIDE`，`closest('[data-tauri-drag-region]')` 从 true 变 false。
- **最小建议**：保留 WindowChrome/原生标题栏，将专用拖动条置于全窗设置之上并按 zoom 换算固定原生高度，避开搜索/按钮；桌面保留一个 Settings 返回出口，子页返回不是关闭设置，应单独保留。当前 worktree 已提高拖动条并删掉右上角重复关闭。
- **重要纠正**：HEAD 的 SettingsModal/WindowChrome **没有自绘三色灯**，其顶部也没有三色灯 DOM。自绘灯在 `src/pages/LessComputerPanel.tsx:598-626`、`src/pages/less-computer-panel.css:227-262`，源于基线前的 `51f6f4cf`，不应报成 `8c32db9e` 新增的 Settings 红绿灯。替换 LessComputer 时应仅收口该窗口的 native decorations/CloseRequested→既有 dismiss 路径，不能只删前端灯造成无关闭入口；这部分 native 实际行为未验证。

### 4. P2：长菜单被强行滚回顶部，真正问题在旧 SelectLite 的高亮初始化

- **位置**：`src/components/ui/SelectLite.tsx:184-202,209-217`；`src/pages/settings/MicrophoneSelect.tsx:65-79,100-119`；新放大因素为 `src/pages/settings/ExperimentalAsrSection.tsx:33-58` 和 `src/components/SettingsModal.tsx:76`。
- **触发**：打开超过视口高度的列表，把鼠标移到较下方选项并继续浏览；父组件有 ASR 状态轮询或麦克风电平更新。
- **原因**：`filteredOptions` 随 options identity 更新，effect 每次都把 highlight 改回 selectedIndex；另一个 effect 又把 highlight 对应项滚进视口。麦克风电平每 120ms 改 level 并重新生成 options。分支又把无条件每秒 `setModels(next)` 的实验 ASR hook 放在整个 SettingsModal 中，即使实验未打开也会触发子树 render，扩大旧故障范围。
- **引入提交**：高亮重置的原始代码为基线前 `66915405`；电平刷新也是旧实现，**不是新 Base UI Select 的 bug**。全 Settings 的每秒刷新由 `16a73b9d` 新增。
- **已验证**：临时 fixture 使用原始真实组件、50 个选项、真实 `useExperimentalAsr(true)` mock 轮询。两个浏览器 Legacy Select 均从 scrollTop 700 在约一秒内回到 4；相同 owner/相同重建 options 下的新 Base UI Select 稳定保持 696。WebKit 的 mock 麦克风列表从 700 下一次 120ms 采样变 4；Chromium 在首采样前就已经回到 4。三种场景都没有 pageerror。
- **最小建议**：普通 selects 复用已有 `@base-ui/react/select` 包装；需要搜索的复用同依赖的 Combobox，补齐现有 disabled/trailing/alwaysVisible/onOpenChange 契约即可。不要再造 wheel 拦截或二次惯性滚动；至少必须把“打开/筛选/值改变初始化”与“选项内容刷新”分离，不要在 hover 或电平更新时 scrollIntoView。当前 worktree 已用 Select/Combobox facade 统一，方向与上述对照验证一致；新 facade 的全部调用契约未逐一动态验证。

### 5. P2：100% 返回录音页的“抽搐”有两个机制，不应笼统称为 zoom 错误

- **位置**：`src/components/SettingsModal.tsx:121,206-208,731-743`；`src/lib/motion.ts:165-192`；`src/pages/settings/RecordingInputSection.tsx:57-64,150-163`；`src/pages/settings/tabs.tsx:54-74`。
- **触发**：100% 界面大小，Settings 从其他分类返回“录音与输入”，尤其返回时原滚动位置较深或 native capabilities 返回较慢。
- **机制**：分类 `key` 使表单重新挂载；caps 每次从 null 起步，桌面热键/启动/远程输入部分要等 effect 补挂；scrollTop 在 passive effect 才归零；整个新内容又先归零透明度再沿 6px 平移，产生分数字素重采样。100% 时 zoom=1，仍会经历上述过程。
- **归因边界**：caps 从 null、passive scroll reset 和 settings 的 useContentMotion 在基线已有（例如 `1ea467a0`、`30e580fc`、`6f9af895`）；本分支加入新的内容/新快捷键结构，但没有证据能把用户机器的全部重复闪烁单独归因给本次 fontScale 改动。
- **已验证**：两浏览器 100% 返回页会重建一个 240ms opacity/translateY 动画，采样到 `translateY(4.29px)` 等分数位移；采样开始后页高分别稳定在 1426/1418，没有复现持续多次抖动或异常。因此这里只确认跳入机制，**未宣称已复现真机 WKWebView 的全部闪烁**。
- **最小建议**：初始化复用 `getCachedPlatformCapabilities`，scroll reset 放 layout effect，页面内容只做 opacity 过渡；不要用调整默认缩放来掩盖。当前 worktree 已做这些主路径修复。native 首帧、macOS 系统字体栅格化及真机100%往返仍需主 Agent 的运行验收。

### 6. P2：快捷键出现重复录制入口，提示/错误内容直接顶开布局

- **位置**：`src/components/ShortcutRecorder.tsx:343-365,386-410,429-433`；`src/styles/refinements.css:261-272`。
- **触发**：点击 Fn 等按键显示进入录制，或展开箭头菜单；录制失败出现错误文案。
- **原因**：按键按钮和展开后的“录制快捷键”都绑定 `startRecording`；录制提示使用 `recording ? ... : ...` 直接替换整个 idle 区域，并没有用新增 Collapse 包住。error 也是裸条件内容。
- **引入提交**：`8c32db9e` 把只显示的按键改成可点击，同时保留菜单同一动作，并移除了旧 AnimatePresence 过渡。
- **已验证**：原始 HEAD 两浏览器的单个 recorder 高度均从 38px 直接变为 59px；内容是“请按下快捷键组合… / Esc · 取消”。这是快捷键捕获的 mock，不是音频录制。
- **最小建议**：保留按键本体为单一录制入口，reset/disable/native dictation key 是独立动作；仅为提示/错误加稳定容器和高度插值/复用 Base Collapsible，保留现有 Fn 原生事件、左右修饰键、校验与保存流程，不重写热键协议。`Fn` 和 `MacDictationKey` 是不同 binding，不能把原生听写键选项当作同义动作删除。
- **当前 worktree 余项**：展开箭头/重复录制已删；但 `src/components/ShortcutRecorder.tsx:302-316` 仍直接条件替换录制区，错误也仍直接条件挂载。未发现其外层已有高度插值，建议作为剩余“提示平滑撑开”复核点；当前新布局未重新测高度。

### 7. P2：实验下载生命周期与窗口生命周期混在一起，0% 取消要区分前后端

- **位置**：`src/components/SettingsModal.tsx:76,764-770`；`src/pages/settings/ExperimentalAsrSection.tsx:20-59,60-76,155-174,208-222`；`src/components/GlobalDownloadProgress.tsx:33,51-153`。
- **触发**：实验模型下载尚未有首字节、关闭 Settings、再打开或想取消。
- **原始实现**：controller 只由 SettingsModal 持有，关闭后轮询和可见控制一同停止；GlobalDownloadProgress 只支持 qwen3/sherpa/foundry，不支持三个 experimental provider。因此下载继续时主窗口没有统一进度和取消入口，恢复后只能等下一次 status 读。下载进度/错误段落裸条件插入，状态和 Toggle 放进 `SettingRow` 默认 `justifyContent:flex-start` 的同一行，状态不是稳定靠右。
- **引入提交**：`16a73b9d` 新增实验控制器/页面，却没有接入已有 global download surface。
- **0% 的证据边界**：`downloadPercent` 会 floor，收到少量字节也可能显示 0%。前端 `toggle(false)` 并不检查 downloadedBytes/percent，也没有把 downloading 状态永久禁用；只有 pending/removing 等状态禁用。不能仅凭此 UI 代码断言 native 0% 请求“不允许取消”，或保证禁用已终止下载/释放进程；这些属于主 Agent 的后端审核。
- **最小建议**：一个共享 controller/store 同时供 Settings 与全局下载区域读取；取消复用 `setExperimentalAsrEnabled(model,false)`，区分等待首字节与已开始的确定进度，显示 pending/error，不把“关闭设置”隐式等同于取消。复用现有全局区域即可，不新增独立下载框架。状态组靠右、Toggle 最右，进度保留固定槽，错误复用 Collapse。
- **当前 worktree**：已看到 `src/lib/useExperimentalAsr.ts` 的共享 useSyncExternalStore、去重更新与 generation 清理；GlobalDownloadProgress 与 Settings 已共同使用，状态右对齐、首字节 indeterminate、错误 Collapse 亦已落地。只做静态复核，未验证真实模型下载/取消、应用重启、native worker 释放或取消残留。

### 8. 要求差距而非新回归：默认 110% 与可用弹层右优先

- **位置**：`src/lib/fontScale.ts:15-28`；`src/components/Tooltip.tsx:120`；`src/components/ui/Select.tsx:30-35`。
- **原始行为**：没保存设置的 Windows 用 large/110%，macOS 用 medium/100%；`Hint` 显式强制 `placement="top"`，覆盖 Tooltip 本身默认 right；新 Base Select 未设 side，因此也不是右优先。
- **归因**：字体默认早于范围（Windows 分支 `9e62dfe2`、medium fallback `48f21341`）；Hint 的 top 和新 Select 为 `8c32db9e`，且原 e2e 故意验证 top。这两项应按用户新要求修改，不应包装成比例计算回归。
- **最小建议**：仅无效/不存在存储值时默认 large，尊重显式90/100%；Hint/Select/Combobox 用 Base UI Positioner `side="right"`，继续依赖 collision handling 在无空间时翻转，保留 portal 的 zoom compensation。当前 worktree 已做。

## 补充：所有主要退出生命周期调用点（协调者追加要求）

共享 `Modal` 的 closing 默认 false，只画 exit 并不会替调用者延迟卸载。以下缺口已通过协调消息发送，主 Agent在本报告写作期间继续修复；表中同时记录原始证据和交付前的最新静态状态。

| 原始调用点（HEAD） | 触发/实际卸载原因 | 引入归因 | 交付前 worktree 复核 |
|---|---|---|---|
| `src/pages/settings/ExperimentalAsrSection.tsx:242,257` | acknowledge 后 ready=[]，直接 return null | 新增 `16a73b9d` | 已改 `:170-171` 为无条件 PresenceModal + open=ready.length>0，保留最后 render closure；未动态复测。 |
| `src/components/CloudSyncSetupPrompt.tsx:114-116` | 用户点稍后/背景/打开设置后 open=false，直接 return null | 基线前 `9731a7f0` | 已改 `:116` 为 PresenceModal。原注释的 Core eligibility revoke 即时撤销语义应保留（动画中不可交互），未验证 native revoke。 |
| `src/pages/settings/CloudSyncSection.tsx:873-879,1435` | `dialog && <SyncDialog key=kind>`，closeDialog 置空；换 kind 也重建 | 基线前 `9731a7f0`/`62e09f1f` | 已在当前 `:878` 用 ExitPresence 保留 SyncDialog，`:1442` 透传 closing；`:865` 的 Github 登录亦有 ExitPresence。业务 busy/恢复流程没有动态验证。 |
| `src/pages/Marketplace.tsx:1600-1601` | showLogin=false 立即卸载 GithubLoginModal，未传 closing | 基线前 `52c49fcf`/`4519f19c` | 已在相同调用点改 ExitPresence 并传 closing；不重挂 OAuth flow 的运行行为仍需验证。 |
| `src/pages/settings/MarketplaceSection.tsx:126-128` | 同上 | 基线前 `52c49fcf` | 当前 `:127` 已 ExitPresence + closing。 |
| `src/pages/LessComputerPanel.tsx:235,796-799` | login.mounted 保留，传 login.closing | 已有实现 | **原本已覆盖**；不要把 Github 所有调用点一概列成缺陷。 |
| `src/components/CloudSyncSetupPrompt.tsx:198` 的 CloudSyncWelcome、`src/components/AutoUpdate.tsx:340`、`src/pages/Marketplace.tsx:923,1125,1277` | PresenceModal 缓存退出内容 | 已有实现 | **原本已覆盖**。 |
| Settings、MobileMoreSheet/MobileStyleSheet、Vocab ModalShell | useExitMount + closing | 已有实现/本轮修复 | 有生命周期，应检查真正合成/内容保留，不是再包一层动画组件。 |
| `src/pages/settings/ChannelList.tsx:1191` 的无 host fallback Modal | ChannelDialog 原主路径在 Settings 使用 right-pane host；fallback 为裸 Modal | 基线前 | Settings 当前主路径非此 fallback，不作为本轮主修边界；若独立复用 fallback，需要与 caller 一起保留 closing。 |

另一个已反馈遗漏：原始 `ExperimentalAsrGuidance` 没有 Escape handler，共享 Modal 原来只处理 Tab，Settings 又对内层 dialog 提前返回，因此 Guide 原先不支持 Escape 关闭。**交付前复核已看到** `src/components/ui/Modal.tsx:66-73` 新增 top-most、defaultPrevented、isComposing、inert 判断并通过 closeRef 调 onClose，此静态缺口已补；子 Select/Combobox 消费 Escape 后不能同时关闭 Guide/Settings 的行为未动态复测。当前 ExitPresence/PresenceModal 只做保留最后 render closure 和透传 closing，改动边界较小。

## e2e 能证明什么、缺什么

- `e2e/motion.spec.ts:140-181` 验证的是 inner Settings surface 存在 exit、起始 opacity 连续，未检查 opaque workspace 是否跟着淡出，所以能漏掉问题2。`:126-140` 的快速返回只检查最终可见/inert，而且按键 Enter 使原始 interaction mode 跳过动画，不能证明鼠标导航有完整退出。
- `e2e/settings-feedback.spec.ts:99-135` 只打开 7 个字体选项，验证键盘 End/Enter 与锚点，没有长列表 wheel + 高频 options refresh。新增回归应使用50项，先 hover下部并滚动，跨越120ms电平或1000ms轮询，确认 scrollTop 不被 selectedItem 抢回。
- `e2e/macos-experimental-asr.spec.ts:94-111` 的关闭再打开测试验证 mock 状态/指导恢复，但 mock 仅在 `getExperimentalAsrStatus` 中按 Date.now 推导进度；它不能证明 native 下载在关闭期间继续、0字节可取消或 worker 退出。
- `e2e/refinements.spec.ts:266-285` 只验证 ExpandableText 最终高度，没有检查 recorder 条件切换或提示关闭时每帧高度，不能证明“平滑撑开”。
- 原测试中有按旧行为刻意写死的断言：`motion.spec.ts:118-119` 要求 Settings y>16/scale<.97；`settings-feedback.spec.ts:53-55,132-134` 要求提示在上方；多文件桌面设置按名称“关闭”退出。用户已要求纯淡出/右优先/单一返回入口，修复后应同步这些 oracle，不应为了绿测恢复旧交互。
- `0947c05a` 自然观察入口完成、`edff8913` 跨平台 UI oracle 修正是测试可靠性改进；它们没有解决以上生命周期/父层遮挡问题。

## 动态证据摘要与未验证项

独立固定 HEAD 临时浏览器结果：

| 探针 | Chromium | WebKit |
|---|---|---|
| Settings 顶部命中 | drag=true → false/ASIDE | 相同 |
| Settings 关闭 | inner≈0.001，outer=1且不透明，然后一起卸载 | 相同 |
| 100% 分类返回 | 240ms 透明+6px平移；观察后页高1426稳定 | 同类动画；页高1418稳定 |
| Shortcut idle→recording | 38px→59px，无高度过渡 | 相同 |
| 主导航 History→Overview | 只有 enter，无 outgoing exit | 相同 |
| 50项旧 SelectLite + 实验轮询 | 700→4 | 700→4 |
| 50项 Base UI Select + 同样轮询 | 696保持不变 | 696保持不变 |
| 50项 mock 麦克风 | 首采样已回4 | 700→4，下个120ms采样发生 |
| pageerror | 无 | 无 |

当前 worktree 的修复只做静态比对；没有把旧 HEAD 的探针结果冒充成修复后验证结果。未验证：Tauri 真机拖动/三色灯位置和 CloseRequested 行为、100%/110%系统字体反复栅格化、真实实验模型首字节等待/取消与子进程回收、OAuth 和云同步完整生命周期、所有搜索 Select 契约以及新 PNG 基线。临时服务器、快照和探针文件已清理；可复现方法是将 HEAD 导出到独立临时目录，用既有 Vite/Playwright 启动 mock 浏览器，记录 Element.animate 的目标/关键帧、elementFromPoint 和 listbox.scrollTop；复核不依赖任何真实服务。
