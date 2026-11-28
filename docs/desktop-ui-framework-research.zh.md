# OpenLess 桌面 UI 框架选型

2026-11-28 · Windows、macOS、Linux · 仓库基准为 `40f5d7d8` 所在工作区。本次核对了源码、发布配置和上游资料，三种新宿主均未做产品实机测试。

## 选型结论

**统一产品界面可行。建议保留 React、TypeScript、CSS，把 Qt 6 + Qt WebEngine 作为第一个验证的新窗口宿主。当前没有可以直接定案、覆盖全部桌面的候选。** Qt 能让同一份页面在三端使用 Chromium 排版，也能通过 KDE 的 LayerShellQt 给单独一个窗口指定 Wayland 图层和键盘交互策略。设置、历史、词库、服务配置和胶囊都可以继续用 React 写，Qt Quick 只需承载一个网页视图。[Qt WebEngine 架构](https://doc.qt.io/qt-6/qtwebengine-overview.html) · [LayerShellQt 6.7 窗口接口](https://github.com/KDE/layer-shell-qt/blob/v6.7.0/src/interfaces/window.h)

**先验证 GNOME 原生 Wayland。** 标准 GNOME/Mutter 不提供上述 layer-shell。普通应用可以显示窗口、设置部分输入区域，但缺少同时指定外部应用上方的位置、层级，并在映射和点击时维持原键盘焦点的公共契约。如果产品必须在未安装扩展的标准 GNOME 上满足全部要求，三案均淘汰。若允许安装随产品维护的 GNOME Shell 扩展，可以验证一条保留 React 胶囊的路径；在它通过前，不应开始主窗口迁移。[layer-shell 上游兼容表](https://github.com/wmww/gtk4-layer-shell#supported-desktops) · [GNOME 对扩展兼容性的说明](https://gjs.guide/extensions/overview/updates-and-breakage.html)

### 三个候选

| 候选和排序 | 谁画界面、谁创建系统窗口 | 怎样接现有 Rust | 选择理由与主要代价 |
|---|---|---|---|
| **第一验证顺位：React + Qt 6 / Qt WebEngine** | 全部产品窗口用同一份 React DOM 和 CSS。普通窗口用 Qt 顶层窗口承载 WebEngineView；胶囊单独创建透明窗口，在相应平台绑定 HWND、AppKit 面板或 layer surface。 | JS 经 Qt WebChannel 调薄的 QObject 命令接口，回包、事件用请求 ID 和信号；Qt 宿主经本地 IPC 接常驻 Rust。 | 三端一套引擎，Qt 有窗口、托盘、菜单、文件对话框；KDE 有上游维护的适配。代价是 Qt/C++、Chromium 分发和 QtWayland 插件版本匹配，macOS 还要验证不激活面板嵌入。 [WebChannel](https://doc.qt.io/qt-6/qtwebchannel-index.html) · [Qt 部署清单](https://doc.qt.io/qtwebengine/qtwebengine-deploying.html) |
| **备选：React + 三端原生 WebView 新宿主** | Windows 用 Win32 + WebView2，macOS 用 AppKit + WKWebView，Linux 用 GTK 4 + WebKitGTK 6.0 API；支持 layer-shell 的 Wayland 在 GTK 窗口首次 realization 前接 gtk4-layer-shell。三端加载同一包 React 资源。 | WebView2 WebMessage、WKScriptMessageHandler、WebKitUserContentManager 转到相同的 Rust 请求、应答和订阅协议。窗口、托盘和系统菜单各有薄适配。 | Windows、macOS 最接近现有网页渲染环境。可减小自带引擎的下载成本，保留 macOS 14.0 的路径也更直接。团队要自持三套窗口嵌入、分发和版本回归，Linux 的 WebKit 与系统字体还要单独验收。 [WebView2](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution) · [WKScriptMessageHandler](https://developer.apple.com/documentation/webkit/wkscriptmessagehandler) · [WebKitGTK 的 GTK 4 API](https://webkitgtk.org/reference/webkit2gtk/2.39.91/migrating-to-webkitgtk-6.0.html) · [GTK 窗口初始化](https://wmww.github.io/gtk4-layer-shell/gtk4-layer-shell-GTK4-Layer-Shell.html) |
| **第三：React + Electron 45** | 普通窗口、Windows/macOS 胶囊优先用 BrowserWindow；桌面特殊行为仍需原生适配。在支持 layer-shell 的 Linux 上，可另起 GTK 4 + WebKitGTK 小宿主，只加载同一份 React 胶囊。 | 隔离的 preload 只暴露业务命令和订阅；Electron 主进程经本地 IPC 接 Rust，Linux 胶囊也接同一会话。 | 普通页面迁移顺手，分发工具成熟。原生 Wayland 的 BrowserWindow 无法兑现 `showInactive`、精确定位及 `setAlwaysOnTop`；额外引入 GTK 胶囊会让 Linux 同时维护 Chromium 和 WebKit 两套运行环境。纯 Electron 窗口方案淘汰。 [BrowserWindow 平台限制](https://www.electronjs.org/docs/latest/api/browser-window) · [Electron 进程模型](https://www.electronjs.org/docs/latest/tutorial/process-model) |

三案都替换 Tauri。原生 WebView 方案直接使用系统嵌入接口。Electron 还有离屏渲染路线。已发布的 electron-overlay 0.5.2 能把 Chromium 画面送到单独的 layer surface，但它的该后端设置空输入区域，整面穿透，还需自行解决可点击控件、读屏和 GNOME。这条现成路径也未通过硬要求。[Electron 离屏渲染](https://www.electronjs.org/docs/latest/tutorial/offscreen-rendering) · [electron-overlay 0.5.2 实现说明](https://github.com/COVAS-Labs/electron-overlay/blob/v0.5.2/packages/electron-overlay/README.md)

改用 Flutter 或纯 Qt Quick/QML 会增加一轮页面、布局与动画重写，同时仍受同一组合成器权限限制，因此未列入前三。

## 能力对照

**自带**表示该层公开提供相应能力，**适配**表示需要应用或平台实现，**待验**表示关键结果尚无本产品证据，**不支持**表示在所列前提下缺少必要能力。下表比较工具，下一张表判断最终用户路径。两表中的“适配”都不代表通过实机验收。

| 能力 | Qt WebEngine | 原生 WebView | Electron |
|---|---|---|---|
| React 组件、DOM、CSS、翻译、网页动画、普通可输入窗口 | 自带 | 自带 | 自带 |
| 多窗口、可输入的 Ask、设置编辑、文件对话框 | 自带窗口和系统对话框；适配路由、返回值 | 自带各系统控件；适配三个宿主 | 自带窗口和系统对话框；适配路由、返回值 |
| 产品内确认框、抽屉、浮层及反馈 | 复用现有 React API；窗口所有权适配 | 同左 | 同左 |
| 托盘、系统菜单、重开主窗口 | 自带 QSystemTrayIcon/QMenu；桌面接收端待验 | 适配 Win32、NSStatusItem、Linux SNI | 自带 Tray/Menu；桌面接收端待验 |
| 独立后台、热键按下/组合/松开/取消、向原光标写回 | 适配现有 Rust 服务 | 适配现有 Rust 服务 | 适配现有 Rust 服务；globalShortcut 单独不支持整套语义 |
| Windows、macOS 胶囊不激活、局部穿透、全屏/多桌面 | 适配；实际 WebEngine 子视图和菜单待验 | 适配；系统面板与嵌入视图合用待验 | 适配；整窗忽略鼠标无法用于可点按钮，局部形状在 Windows/Linux 为实验接口 |
| 原生 X11 胶囊 | 适配 X11 | 适配 GDK/X11 | 适配 X11 |
| 原生 KWin、Sway、Hyprland 胶囊 | 适配 LayerShellQt | 适配 gtk4-layer-shell | BrowserWindow 单独不支持；另配 GTK 胶囊待验 |
| 原生 GNOME Wayland 完整胶囊 | 默认不支持；Shell 扩展待验 | 默认不支持；同项待验 | 默认不支持；同项待验 |
| 高 DPI、可访问性、长文本 | 网页排版和辅助技术入口自带；系统缩放、读屏和胶囊待验 | 引擎提供入口；三套系统组合待验 | Chromium 提供入口；胶囊及 Linux 小宿主待验 |
| 首个实际可见帧、 renderer 崩溃恢复、旧会话隔离 | 适配并待验 | 适配并待验 | 适配并待验 |

Qt 的 `QWindow::setMask` 是对窗口系统的提示，平台也可以同时裁掉可见像素；Electron 的 `setShape` 同样同时限制绘制与交互。GDK 提供独立输入区域。CSS 的透明度和 `pointer-events` 只能处理本视图内的命中。需要让窗口系统把空白处的点击直接交给另一进程，并保留胶囊文字、阴影和按钮的画面。[Qt 窗口遮罩](https://doc.qt.io/qt-6/qwindow.html#setMask) · [Electron 窗口形状](https://www.electronjs.org/docs/latest/api/browser-window#winsetshaperects-windows-linux-experimental) · [GDK 输入区域](https://docs.gtk.org/gdk4/method.Surface.set_input_region.html)

Windows 上单靠 `HTTRANSPARENT` 还有同线程命中范围限制，须用另一进程的真实窗口测穿透。macOS 现有 Web 胶囊会轮询指针位置并切换整窗忽略鼠标，指针快速跨界时有首击竞态。新宿主可试让视觉面板穿透，只给 DOM 上报的按钮和展开菜单配置可收第一击的非激活小面板，操作回到同一份 React 状态；该设计的圆角边缘、滚动手势和读屏仍须实机证明。[Win32 命中语义](https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-nchittest) · [现有 macOS 命中实现](../openless-all/app/src-tauri/src/macos_capsule_input.rs) · [AppKit 面板键盘策略](https://developer.apple.com/documentation/appkit/nspanel/becomeskeyonlyifneeded)

### 各桌面的胶囊和输入链路

表中版本是建议验证的系统样本。每行都要求透明无边框、无独立任务栏项，且出现、点击按钮、开关操作菜单和收起四个时刻均保留目标的键盘焦点。

| 系统与版本 | 可见、定位、层级与全屏 | 不激活；按钮可点和空白穿透 | 输入法、全局热键与原光标写回 |
|---|---|---|---|
| **Windows 10 x64（1809、22H2）及 Windows 11** | **适配**。无边框 tool window、topmost、无激活显示；按目标屏工作区放置。普通全屏、独占全屏和虚拟桌面切换分别**待验**。 | **适配**。创建时设 `WS_EX_NOACTIVATE / TOOLWINDOW`，无激活提升，点击用 `MA_NOACTIVATE` 保留鼠标事件。真实 layered window 的零 alpha 像素可穿透；带 Chromium 子窗口时须验证原生命中，必要时分开视觉与按钮区域。 | **适配**。保留 Rust 低层键盘钩子、原目标及 TSF/现有插入路径。验证同应用的两个输入框、普通/提升权限边界、系统 DPI 改变后的第一击。 [Win32 窗口样式](https://learn.microsoft.com/en-us/windows/win32/winmsg/extended-window-styles) · [鼠标激活](https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-mouseactivate) · [分层窗口命中](https://learn.microsoft.com/en-us/windows/win32/winmsg/window-features#layered-windows) |
| **macOS 14.0、14.4 及 26，Apple Silicon / Intel 的适用机型** | **适配**。透明无边框 NSPanel，取合适的 window level，按现有策略加入 Spaces，并设置 fullScreenAuxiliary；跨应用全屏 Space、切桌面和外接屏分别**待验**。 | **适配，局部命中待验**。创建时指定 nonactivatingPanel，控件接收 first mouse、不要求成为 key；在同一面板内画菜单。整窗 ignoresMouseEvents 会连按钮都放过。验证快速从空白移到按钮立刻单击，以及首次点击后继续在原框打字。 | **适配**。沿用 CGEventTap、适用的 Fn/左右修饰键和原 AX/输入路径。按实际负责进程验证麦克风与辅助功能等授权，安装升级后复验。 [不激活面板](https://developer.apple.com/documentation/appkit/nswindow/stylemask-swift.struct/nonactivatingpanel) · [全屏辅助窗口](https://developer.apple.com/documentation/appkit/nswindow/collectionbehavior-swift.struct/fullscreenauxiliary) · [Spaces](https://developer.apple.com/documentation/appkit/nswindow/collectionbehavior-swift.struct/canjoinallspaces) |
| **原生 X11，Xorg 21.1，Xfce/Xfwm 4.18 或 4.20** | **适配**。使用带 X11 后端的 Qt xcb/GDK X11 或 Electron X11、ARGB 视觉和开启合成的 WM，RandR 选屏，EWMH ABOVE、SKIP_TASKBAR、工作区提示。窗口管理器决定实际叠放；全屏放在下方、混合缩放模糊都需记录。 | **适配**。映射前声明 ICCCM 不接收键盘输入，胶囊及菜单不请求焦点。SHAPE 1.1 的 ShapeInput 只留实际交互区域。验证不同 WM 焦点策略；事后夺回焦点即失败。 | **适配**。安装启用 Fcitx5 和 OpenLess addon，让目标应用经 XIM/GTK/Qt 相应前端连接，验证按键边沿与捕获的输入上下文。 [EWMH 1.5](https://specifications.freedesktop.org/wm/latest/) · [SHAPE 1.1](https://www.x.org/releases/X11R7.7/doc/xextproto/shape.html) · [Xfwm 合成设置](https://docs.xfce.org/xfce/xfwm4/wmtweaks) |
| **GNOME/Mutter 原生 Wayland，46、48、50、51** | **标准会话不支持完整胶囊**。普通 xdg-toplevel 能显示，但不能指定屏幕绝对位置和所需全屏上层；Mutter 未提供 wlr-layer-shell。扩展托管同一 React 胶囊时，这些项均**待验**。 | 原生 surface 可有输入区域；标准顶层缺少全程不激活的保证。**有扩展仍待验**，尤其是第一帧映射、第一次点菜单和窗口销毁。 | **适配，逐应用待验**。默认 IBus 不能直接提供 OpenLess 的 Fcitx addon 接口；需实际启用 Fcitx5、其 IBus 前端或相应 toolkit IM 模块，并确认目标经该路径。GNOME 48 起支持 GlobalShortcuts portal，46 不享有此项上游保证。portal 也不提供原光标。 [GNOME 48 发布说明](https://release.gnome.org/48/developers/) · [Fcitx 的 GNOME 配置](https://fcitx-im.org/wiki/Using_Fcitx_5_on_Wayland#GNOME) · [兼容表](https://github.com/wmww/gtk4-layer-shell#supported-desktops) |
| **KDE Plasma / KWin 原生 Wayland，5.27.11、6.7、6.8** | **适配**。上游 KWin 5.27 已有 layer-shell；绑定目标 output、底边锚点、overlay 层、零保留区。普通全屏、跨桌面以及合成器实际选屏**待验**。 | **适配**。只给胶囊 `keyboard_interactivity=none`；同一 layer surface 内画菜单，按按钮和菜单视口提交输入区域。 | **适配**。用 Plasma 的虚拟键盘设置启动 Fcitx5，启用 OpenLess addon，分别测原生 GTK、Qt 和浏览器目标的输入上下文。旧 KWin 可理解协议，客户端仍须提供与自身 Qt 匹配的插件。 [KWin 5.27.11](https://github.com/KDE/kwin/blob/v5.27.11/src/layershellv1integration.cpp) · [KWin 6.7](https://github.com/KDE/kwin/blob/v6.7.0/src/layershellv1integration.cpp) · [Plasma 6.8](https://kde.org/announcements/plasma/6/6.8.0/) · [Fcitx 的 KDE 配置](https://fcitx-im.org/wiki/Using_Fcitx_5_on_Wayland#KDE_Plasma) |
| **Sway 原生 Wayland，1.10、1.11** | **适配**。使用 overlay layer；用 Sway IPC 的聚焦容器、工作区与 output 验证前台屏选择，分别测普通和全局全屏。 | **适配**。图层拒绝键盘输入，鼠标按 surface-local region 分发。菜单不能建立键盘 grab；缩放和工作区切换后的命中**待验**。 | **适配**。Fcitx5、OpenLess addon、text-input-v3/input-method-v2；1.10 是 Fcitx 官方给出含候选弹窗改进的基线。常用 xdg-desktop-portal-wlr 只实现截图和录屏，不能用它替代热键监听。 [Sway 1.10](https://github.com/swaywm/sway/releases/tag/1.10) · [Sway IPC](https://github.com/swaywm/sway/blob/1.10/sway/sway-ipc.7.scd) · [Fcitx 的 Sway 配置](https://fcitx-im.org/wiki/Using_Fcitx_5_on_Wayland#Sway) · [wlr portal](https://github.com/emersion/xdg-desktop-portal-wlr) |
| **Hyprland 原生 Wayland，0.50、0.57** | **适配**。这两个版本分别作较旧和较新样本。按所通告的 layer-shell 创建；通过合成器 IPC 核对 activewindow、monitor 与光标所在屏。图层规则、特殊工作区、全屏**待验**。 | **适配**。overlay + 无键盘交互 + 按控件设输入区域。检查用户 layer 动画、缩放或 blur 规则生效时，画面和点击位置仍匹配。 | **适配**。上游实现 input-method-v2、text-input-v3；会话须启动 Fcitx5 并加载 OpenLess addon。原生目标是否实际经 Fcitx 收到组合、松开及 commit 需逐项验证。 [0.50 的协议实现](https://github.com/hyprwm/Hyprland/blob/v0.50.0/src/managers/ProtocolManager.cpp) · [0.57 发布](https://github.com/hyprwm/Hyprland/releases/tag/v0.57.0) · [Hyprland IPC](https://wiki.hypr.land/0.50.0/configuring/using-hyprctl/) |

**Wayland 的首次映射和选屏。** 在创建时给新 surface 分配图层角色，先无 buffer 提交，收到 configure 并 ack 后才附上第一帧。输入 region 使用 surface 逻辑坐标，同菜单和动画帧一起更新。无可点控件时提交显式空 region；Wayland 把 null region 解释为整面接收输入。传空 output 表示由合成器选择，不能据此承诺“前台窗口屏”等于“鼠标屏”。OpenLess 已有主显示器、前台窗口屏、光标屏和全部屏四种模式；须分别取得相应桌面的选屏依据或为每块目标屏建镜像，镜像共享一个会话与操作所有者。每一项无法实现的显示模式都应明确登记。原生 Wayland 的验证使用原生目标和原生胶囊，XWayland 客户端只作补充兼容项。[layer-shell 协议](https://github.com/swaywm/wlr-protocols/blob/master/unstable/wlr-layer-shell-unstable-v1.xml) · [Wayland 的 surface 输入区域](https://wayland.freedesktop.org/docs/html/apa.html#protocol-spec-wl_surface-request-set_input_region) · [项目多屏规则](../openless-all/app/src-tauri/src/capsule_mirrors.rs)

**GNOME 扩展要证明的事。** 原型只让扩展管理专属胶囊客户端的身份、映射时序、输出、层级与输入区域，产品画面、菜单和操作仍由原 React 组件产生。Mutter 有识别受托 Wayland 客户端窗口的接口，Shell 也能管理最上层 actor 和输入区域；两者之间如何可靠地承载可交互 WebView，尤其如何在首次映射前保证不激活，仍是未解决的集成点。扩展需要随包或经 GNOME Extensions 安装、显式启用，在 metadata 声明并逐个回归 Shell 主版本；禁用扩展的配置应单列为不支持。若必须复制一套 GJS 产品组件、传输画面时丢失操作或读屏语义，或长期分叉 Mutter，淘汰这条路径。Kimpanel 解决输入法候选窗，不能当此原型的成果。[Mutter WaylandClient](https://mutter.gnome.org/meta/class.WaylandClient.html) · [GNOME Shell 48 的层级和输入实现](https://github.com/GNOME/gnome-shell/blob/gnome-48/js/ui/layout.js) · [GNOME 扩展版本规则](https://gjs.guide/extensions/overview/updates-and-breakage.html)

**窗口角色与热键。** 胶囊、镜像和展开的操作菜单走同一条不激活路径；菜单键盘导航沿用当前会话的定向按键处理，其他普通按键继续到目标。主窗口、Ask 输入、设置、热键录制、文件选择则正常获取键盘焦点。保留现有 Windows 钩子和 macOS 事件监听的物理键边沿、左右修饰、Fn、自动重复抑制、组合取消、长按/切换、会话内 Shift 手势及注册冲突反馈，按平台现有绑定适用。Linux 移出旧 UI 的 Fcitx DBus 监听负责配对 down/combined/up 与 press ID，写回沿用捕获的 target ticket。GlobalShortcuts v1 有 Activated/Deactivated，但只针对用户获准绑定的动作，不承诺所有裸修饰、左右键、组合打断或目标应用的输入上下文。目标没有接入 Fcitx 的情况下，已有 addon 路径无法承诺全局等价。[现有热键适配](../openless-all/app/src-tauri/src/hotkey.rs) · [Linux 边沿配对](../openless-all/linux/egui/src/hotkeys.rs) · [Fcitx 原目标提交](../openless-all/linux/fcitx5-plugin/openless.cpp) · [GlobalShortcuts v1](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.GlobalShortcuts.html)

### 版本与安装范围

- **Windows、macOS。** 现有发布是 Windows x64、macOS Intel 和 Apple Silicon，应用声明 Windows 10+ 和 macOS 14.0+。Qt 6.12 列出的下限为 Windows 10 1809 和 macOS 14.4，且 6.12 是 Qt 支持 Windows 10 的最后一代。建议把 1809/22H2 和 Windows 11 纳入验证；更早的 Windows 10 构建是 Qt 路线的覆盖差额。macOS 14.0–14.3 也必须保留为待决项。Qt 6.11.3 + WebEngine 6.140.0 是可试的兼容构建，上游 6.140.0 预编译包只面向 Qt 6.12.0，旧线需要云端自行构建、维护补丁；若成本不可接受，转原生 WebView 备选。Electron 45 上游支持 Windows 10+、macOS 13+。Windows 的虚拟桌面 API 不提供通用“钉在所有桌面”承诺；若要求覆盖所有独占全屏，三案也没有通用系统保证，应单独保留验收结论。 [现有宿主声明](../openless-all/app/src-tauri/tauri.conf.json) · [项目平台范围](architecture.md) · [Qt 6.12 平台表](https://doc.qt.io/qt-6.12/supported-platforms.html) · [Qt 6.11 平台表](https://doc.qt.io/qt-6.11/supported-platforms.html) · [WebEngine 6.140.0 兼容表](https://code.qt.io/cgit/qt/qtreleasenotes.git/about/qtwebengine/6.140.0/release-note.md) · [Electron 45 平台表](https://github.com/electron/electron/blob/v45.0.0/README.md#platform-support) · [Windows 虚拟桌面](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nn-shobjidl_core-ivirtualdesktopmanager)
- **Linux 发行。** 延续现有 amd64 deb/rpm 形态。首轮覆盖 Ubuntu 24.04 LTS 和较新的 Fedora，再用有对应桌面的镜像覆盖 Xfce/Xorg、GNOME、KWin、Sway、Hyprland。把 Debian 12 / Ubuntu 22.04 作为较旧 ABI 验证档。Qt 官方在线安装器的 Linux 二进制基于 glibc 2.39，较旧发行版需要匹配发行包或从较旧基线在 CI 构建。发行版能启动主窗口，不等于该发行版默认桌面已通过胶囊验收；例如较早 GNOME Shell 还要另做扩展移植。 [Qt Linux 二进制基线](https://doc.qt.io/qt-6.12/supported-platforms.html#linux-x11) · [项目 deb/rpm 打包](../openless-all/linux/scripts/package-linux-egui.sh)
- **Linux 依赖。** Qt 方案随包提供或依赖 QtWayland 和与 Qt 匹配的 LayerShellQt；以 6.7.0 为样本，该插件需要 Qt ≥6.10 并使用 QtWayland 私有开发接口，升级 Qt 后要重建、回归。只在胶囊首次显示前配置 `LayerOverlay`、`KeyboardInteractivityNone`、`activateOnShow=false`；若采用进程级 shell 选择，仅对独立胶囊进程生效。GTK 方案及 Electron 的 Linux 胶囊需 GTK 4、受维护的 WebKitGTK 6.0 API 运行库和 gtk4-layer-shell 1.x（接口样本 1.0.4），缺包时须补包；每个合成器都按运行时通告的协议版本协商。所有 Linux 方案都另需 Fcitx 5、DBus 和本项目 addon（项目版本 1.0.0），按发行版 Fcitx ABI 安装；GNOME 扩展又是单独一项安装条件。 [LayerShellQt 构建依赖](https://github.com/KDE/layer-shell-qt/blob/v6.7.0/CMakeLists.txt) · [窗口创建时序](https://github.com/KDE/layer-shell-qt/blob/v6.7.0/src/interfaces/window.cpp) · [gtk4-layer-shell 1.0.4](https://github.com/wmww/gtk4-layer-shell/blob/v1.0.4/README.md) · [OpenLess addon 构建依赖](../openless-all/linux/fcitx5-plugin/CMakeLists.txt)
- **托盘与菜单。** 窗口宿主可以发布托盘图标，桌面还须有接收端。GNOME 的标准配置可能需要 AppIndicator/KStatusNotifierItem 扩展；Sway、Hyprland 上要有启用 tray 的面板，如 Waybar。缺接收端时托盘项计为缺失。实际点击菜单、重开主窗口、退出都要验证。Windows 保留 TSF 安装注册，macOS 保留签名包身份与授权，系统 WebView2 缺失时由安装器补 Evergreen Runtime。 [Qt 托盘](https://doc.qt.io/qt-6/qsystemtrayicon.html) · [GNOME 托盘扩展](https://extensions.gnome.org/extension/615/appindicator-support/) · [Waybar tray 模块](https://github.com/Alexays/Waybar/wiki/Module:-Tray) · [WebView2 分发](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution)

## 实际能复用多少

静态盘点范围为 Git 跟踪的 `openless-all/app/src` 和 `ask-window`，排除测试与类型声明，包含这两个目录中的移动端共用代码、预览和薄转发文件。共 **208 个 TSX、255 个 TS、48 个 CSS**。直接静态或动态导入 `@tauri-apps/*` 的有 **2 个 TSX 和 13 个 TS**。`src/features` 下 **105 个 TSX 均无直接 SDK 导入**，其中 11 个仍含显式宿主判断或 Tauri 事件相关标记。业务页面整体沿用有源码依据；静态计数不能换算成工期，也不能推导出“97% 原样运行”。[功能组件](../openless-all/app/src/features) · [运行时边界](../openless-all/app/src/runtime) · [独立 Ask 入口](../openless-all/app/ask-window)

| 迁移类别 | 可以沿用的资产 | 具体工作 |
|---|---|---|
| **原样保留** | 纯 React 布局/控件、TS 格式化/状态投影、CSS 栅格与主题 token、图标、8 种语言资源和插值、与窗口无关的 Motion/keyframes。设置、历史、词库、服务配置均在原页面上继续开发。 | 固定相同的路由、操作名称、反馈语言、错误态和首帧语言。词库/历史的编辑删除、服务配置保存/凭据和模型下载都走真实服务验收。 [主题 token](../openless-all/app/src/styles/tokens.css) · [语言目录与加载](../openless-all/app/src/i18n/index.ts) |
| **原样保留业务，替换接口** | `openless-core`、DSP/推理、录音/ASR、文本处理、历史与偏好存储、各平台原目标写回的业务规则。部分真实实现仍在 `src-tauri` 或 `linux/egui` 目录。 | 保留业务函数与数据格式，移出或替换 `AppHandle`、事件派发、主线程调度、路径与系统资源获取。移植 TSF、macOS 输入、Fcitx 插件和 target ticket 周围的宿主接线；不把录音或插入改写为网页能力。 [Core 端口](../openless-all/app/crates/openless-core/src/ports.rs) · [宿主适配](../openless-all/app/src-tauri/src/core_adapters.rs) · [Linux 写回](../openless-all/linux/egui/src/fcitx5.rs) |
| **替换接口，保留页面** | `runtime/transport`、域事件、`lib/ipc`、AskClient、文件/资源 URL、更新器、主题同步、热键录入、第一帧通知、窗口焦点及拖动区域。现有产品弹窗已有可编程 interaction API。 | 实现新宿主握手、定向命令/订阅/退订与错误映射。接好 Ask 单独的事件通道和窗口权限，为 Linux 新增 OS 与能力声明。旧 Linux 能力表仅在 X11 宣告 supports_overlay，即使另一路能探测到 layer-shell；须让设置页、热键派发和实际窗口使用同一份运行时探测结果。当前 OS 类型没有 Linux，未识别的桌面会落到 Mac，服务就绪失败也不能落入预览假数据。沿用对话框的调用、参数、结果与销毁，再接原生文件选择等。 [环境识别](../openless-all/app/src/runtime/environment.ts) · [Linux 能力表](../openless-all/linux/egui/src/capabilities.rs) · [传输](../openless-all/app/src/runtime/transport.ts) · [AskClient 适配](../openless-all/app/ask-window/tauri-client.ts) · [交互控制器](../openless-all/app/src/ui/overlays/interactionController.ts) |
| **胶囊保留产品实现，重写呈现接线** | 已有 React Capsule、转录条、样式/翻译菜单、SiriGL、按钮、几何和 DOM 命中区域计算；另有 Rust 的 Frame、Action、就绪握手、session/sequence。 | Win/mac 有 Web 胶囊，也有低内存模式用的独立 egui 胶囊。把两条路径的行为逐项对齐到共享 React 组件，以新宿主管首帧、命中、定时收起、跨屏镜像和菜单。现有 TS 的命中区发送仅对 macOS Tauri 生效。原生 egui 绘制、WGPU event loop 不能原样当 Web 组件。 [React 胶囊](../openless-all/app/src/features/capsule/Capsule.tsx) · [命中区发送](../openless-all/app/src/runtime/windows/capsuleInput.ts) · [跨进程胶囊协议](../openless-all/app/crates/openless-capsule/src/lib.rs) · [原生胶囊分支](../openless-all/app/src-tauri/src/tauri_coordinator_host.rs) |
| **必须替换** | Tauri 窗口工厂、托盘菜单装配、事件循环、安装/更新接线；Linux egui 页面、独立弹窗绘制和与 egui view model 绑定的表现层。 | Linux 打开相同 React 路由；把 Linux 独有的 Fcitx 配置/诊断、服务能力与设备状态补进共享组件。保留原 Linux 常驻服务、DBus、音频、设备能力与输入适配。已有 layer-shell runner 可借用探测与几何经验，目前采用 Top 层、默认 output，且无协议时胶囊进程返回错误；原生 X11 路径还带焦点恢复逻辑，均不能当成新体验的合格基线。 [Linux 表现层](../openless-all/linux/egui/src/ui/frontend) · [Wayland runner](../openless-all/linux/egui/src/popup_layer.rs) · [无协议时的行为](../openless-all/linux/egui/src/main.rs) · [X11 路径](../openless-all/linux/egui/src/popup_window.rs) |

### 后台生命周期和迁移次序

1. **先固定可观察契约。** 用仓库 2.0.0 启动快照、语义事件与兼容基线对账。现有清单有 204 个命令、30 个旧事件，另列 34 个 Core 事件种类；它是盘点起点，另核之后新增的 Ask、窗口定向事件、系统菜单和资源协议。记录设置、历史、词库、服务配置、托盘、辅助窗口、热键各自的成功、取消和失败结果。[后端契约](../openless-all/app/contract/backend-2.0.json) · [Linux 命令事件基线](linux-egui-command-event-baseline.json)
2. **让 Rust 是会话的唯一所有者。** 由单实例常驻 Rust 进程持有音频、热键、输入目标、任务和持久化，管理可重启的 UI 宿主/胶囊。用同用户的本地管道，或 Unix socket/Windows named pipe，沿用 Linux JSONL v2 的请求、快照和序号经验。确认主窗口隐藏、销毁、重新启动都不影响正在录音或收尾的任务。需要 GUI 线程或系统消息泵的原生监听留在常驻线程；UI 宿主退出策略、托盘和系统“退出”分别定义。渲染进程重连先补快照/增量，不重发一次性写回。 [Linux UI 进程协议](../openless-all/linux/egui/src/ui/bridge.rs) · [当前主窗口生命周期](../openless-all/app/src-tauri/src/desktop_resources.rs) · [Qt 最后一个窗口的退出策略](https://doc.qt.io/qt-6/qguiapplication.html#quitOnLastWindowClosed-prop)
3. **先迁一条完整听写，再迁页面。** 按键时捕获原输入目标与会话；UI 订阅后先呈现当前快照，再按序号接增量。音量可合并，文本、终态和动作保序。显示、关闭回调、菜单动作、镜像和延迟定时器带会话身份与呈现代数，后台再次检查所有者，允许 A 继续处理而 B 已开始录音。保持原有插入结果分类；原目标已失效或只复制到剪贴板，不记作成功写回。 [会话与收起代数](../openless-all/app/src-tauri/src/native_capsule.rs) · [原目标与写回结果契约](../openless-all/app/contract/backend-2.0.json)
4. **按功能切换，有退出条件。** 先比较两种宿主读取同一份样本数据的结果，切换时只有一个宿主持有热键、麦克风、托盘和写库权。保持应用 ID、存储目录、钥匙串/凭据键、升级与卸载行为；旧 Web origin 的 localStorage（例如语言与主题）要显式迁入新 profile。弹窗、Ask、语音编辑、选区意图、插入失败卡、词库建议、转录条逐一走到最终结果。按窗口角色保留正常的文本输入。新宿主通过兼容清单和实机矩阵后，再停用旧 Linux 产品 UI。

## 视觉、性能和维护成本

**外观和无障碍。** 同一套 DOM 可保留布局和动画参数。Windows 的 DirectWrite、macOS 的 Core Text、Linux 的系统字体与 fontconfig/FreeType，会让 CJK/emoji 回退、字宽、字重和基线不同。以真实字体检查德语长标签、中文/日文长转录、列表和表单，核对 100%–200% 字号与窗口缩放；空间不足时控件可换行、菜单可滚动，关键操作和状态仍可见。比较浅/深色、高对比、减少动态效果及大字时的实际画面。主窗口和 Ask 要可键盘操作；胶囊要能读出状态、识别和执行操作，同时保持原编辑器的键盘焦点。使用 Narrator/NVDA、VoiceOver、Orca 各走一次。Qt WebEngine 的网页可访问性还依赖 Qt 构建与平台辅助技术激活。[Qt 可访问性条件](https://doc.qt.io/qtwebengine/qtwebengine-platform-notes.html#accessibility-and-performance) · [Electron 可访问性](https://www.electronjs.org/docs/latest/tutorial/accessibility) · [GTK 与 Orca](https://docs.gtk.org/gtk4/section-accessibility.html)

**透明、模糊和动画。** 桌面透底、网页内部的 CSS backdrop-filter、桌面合成器采样背后的其他窗口是三个独立效果。macOS 可接 NSVisualEffectView，Windows 11 的 Mica 采样壁纸、呈不透明材质，玻璃效果要另测系统支持的背景材质；Windows 10、X11 合成开关、KWin/Hyprland 特效配置、Sway/GNOME 之间不能承诺相同外部模糊。无模糊时用已有主题的半透明底色保可读性，胶囊外部继续真正透明、可穿透；若产品要求背景模糊逐像素一致，此项仍有缺口。CSS/Motion、WebGL 着色器沿用后，仍要观察冷启动编译、透明边缘黑边、隐藏视图节流、跨显卡和首帧丢失。按 PMv2/WM_DPICHANGED、AppKit points/backing scale、Wayland logical coordinates/output scale 分别更新图像与命中区域。一般合成窗口内可用的模糊替代不能掩盖黑底、透明空白拦截点击等胶囊失败。[Apple 原生视觉效果](https://developer.apple.com/documentation/appkit/nsvisualeffectview) · [Microsoft Mica](https://learn.microsoft.com/en-us/windows/apps/design/style/mica) · [Windows 多屏 DPI](https://learn.microsoft.com/en-us/windows/win32/hidpi/high-dpi-desktop-application-development-on-windows) · [Qt 高 DPI](https://doc.qt.io/qt-6/highdpi.html)

### 性能证据

本轮没有三候选在相同硬件、相同 OpenLess 工作负载下的冷启动、首帧、内存、CPU/GPU 或帧时间实测。仓库有 Windows 冷启动脚本和 Linux 帧统计探针，可作为旧版对照；脚本存在并非性能结果。下表是根据进程和渲染方式做的**工程预期**。[旧版冷启动计时](../openless-all/windows/scripts/windows-cold-start.ps1) · [现有帧统计探针](../openless-all/linux/egui/src/frame_stats.rs)

| 指标 | Qt WebEngine | 原生 WebView | Electron |
|---|---|---|---|
| 冷启动；热键到胶囊可见 | 首次启动 Qt、WebEngine 辅助进程和网页；预备胶囊可缩短热路径 | 冷时仍需启动系统 Web 内容进程。Windows 运行库是否已热、Linux WebKit 首帧会改变结果 | 启动 Chromium/Node 主进程和 renderer；Linux 冷胶囊还可能首次启动 WebKit |
| 空闲/录音内存 | 常驻 WebEngine 的成本与销毁页面后重开的成本需权衡 | 共享引擎可能降低增量；每个 WebView 的内容进程、GPU 缓冲仍占内存 | 保留 renderer 会增加常驻内存；Linux 双引擎同时驻留尤其值得测 |
| 空闲/录音 CPU、GPU、动画 | 主要 UI 变量为透明合成、WebGL、音量更新、输出数量及后端驱动 | Windows/Mac 较接近现有引擎；Linux WebKit 的 GPU 驱动、软件回退和 blur 需要独测 | Chromium 合成可复用现有 Motion；Linux 小宿主改变绘制路径。三案空闲时都应停止不可见动画 |
| 安装与更新 | 分发 Qt、WebEngineProcess、Chromium 资源、翻译及平台插件，整包和补丁均要量 | 已有系统引擎时应用下载有望最小；全新 Windows/Linux 机器须另计引擎、GTK 和插件下载 | 要随应用分发 Electron runtime；Linux 额外安装的 GTK/WebKit 也要计入首装 |

**可核查的体积。** Electron 45.0.0 官方 x64 runtime ZIP 标示 Windows 151 MB、macOS 119 MB、Linux 118 MB，均未包含 OpenLess、Rust、模型和安装器。Ubuntu 24.04 的 amd64 WebKitGTK 单个库包 2.52.6 标示下载约 24,721 kB、安装约 92,344 kB，另有 JavaScriptCore 等依赖。Qt 官方列明须分发的组件，最终 Qt 6.140 OpenLess 包体仍待出包测量。这些数值口径不同，不能据此推算应用内存或启动时间。 [Electron 固定版本发布资产](https://github.com/electron/electron/releases/expanded_assets/v45.0.0) · [Ubuntu WebKitGTK 包清单](https://packages.ubuntu.com/noble/libwebkitgtk-6.0-4) · [Qt 部署文件](https://doc.qt.io/qtwebengine/qtwebengine-deploying.html)

**实测口径。** 同机、同 release 配置、同内容/字体/模型/驱动，旧版与候选交替采样。分开测进程启动到后台可听写、主窗可交互、物理热键到**屏上首个正确像素**，以及胶囊冷建、已预热、第二轮三种情况。记录 p50/p95、样本数、帧时间与掉帧；缓存冷暖分别报告。分别测后台无窗、主窗空闲、录音含实时转录、收尾和收起后的整个进程树；记录 CPU、GPU、显存与 Windows 私有工作集、macOS physical footprint、Linux PSS。在同一系统内比较；各端都跑相同的远程识别负载，已提供本地识别的平台再单测本地模型，分清 UI 与模型成本。安装包注明压缩体积、已安装体积、新机器的额外依赖、更新下载量；模型单列。预热策略由热键首帧改善和额外空闲占用共同决定。

### 分发、许可证与故障隔离

- **Qt。** 项目现有 AGPL-3.0-only。Qt WebEngine 的 Qt 部分有 LGPLv3/GPL/商业许可选择，Chromium 第三方声明另行随包；LayerShellQt 按其源码许可分发。可选开源动态链接方案；发布时处理对应许可证、源码及替换库义务。Qt 6.12 起 WebEngine 独立发布，首个独立版是 6.140.0；LTS、开源补丁可用性和 WebEngine 安全更新节奏要分别管理。Win/mac 的 Qt 部署工具负责收集组件，还需要项目的签名、公证、MSI/NSIS、IME 钩子与更新通道。现有 DMG/Homebrew 路线可规划延续；Qt 官方另列 Mac App Store 限制。 [项目许可证](../LICENSE) · [Qt WebEngine 许可](https://doc.qt.io/qtwebengine/qtwebengine-licensing.html) · [Qt 更新节奏](https://www.qt.io/blog/new-release-cadence-for-qt-webengine-and-qt-pdf) · [Qt 平台说明](https://doc.qt.io/qtwebengine/qtwebengine-platform-notes.html)
- **原生 WebView。** Windows Evergreen、macOS 系统 WebKit、Linux 发行版 WebKitGTK 分别更新，应用包小的代价是测试不同引擎版本；企业固定运行库也要单计空间和更新责任。WebKit 含 LGPL/BSD 部分，gtk4-layer-shell 是 MIT，WebView2 遵守微软分发条款。GTK/WebKitGTK 和系统供应商持续维护引擎，团队维护嵌入代码、原生托盘和安装更新。 [WebKit 许可](https://webkit.org/licensing-webkit/) · [GTK layer-shell 许可](https://github.com/wmww/gtk4-layer-shell/blob/main/LICENSE) · [WebKitGTK 更新](https://webkitgtk.org/security.html) · [WebView2 分发](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution)
- **Electron。** Electron 为 MIT，随包保留 Chromium/Node 等第三方声明；上游按大版本轮替支持，需持续升级。内置 autoUpdater 覆盖 Windows、macOS，且 Windows 需匹配受支持的安装机制；现有 MSI/NSIS 更新链不能直接假定兼容。Linux 优先发行版包管理，deb/rpm 同时处理 Fcitx addon 更新。 [Electron 许可](https://github.com/electron/electron/blob/v45.0.0/LICENSE) · [发布支持日历](https://releases.electronjs.org/schedule) · [官方更新器](https://www.electronjs.org/docs/latest/api/auto-updater)
- **故障边界。** 三案都有 Web 内容进程；Rust 独立常驻后，杀掉主窗、renderer 或胶囊宿主，不应停止录音、重启识别或重复插入。Rust 自身崩溃时如实结束或恢复持久化状态，不能将未确认的插入自动重放。优先使用上游插件和公开嵌入接口；LayerShellQt 私有 ABI 匹配、GNOME 扩展逐版本适配是可见的长期成本。若稳定运行需永久维护 Chromium/QtWebEngine/Mutter 分叉，该候选失去采用理由。 [Qt 多进程部署](https://doc.qt.io/qtwebengine/qtwebengine-deploying.html#deploying-qt-webengine-processes) · [Electron 进程边界](https://www.electronjs.org/docs/latest/tutorial/process-model)

## 决定前的最小实机验证

第一轮只需真实 Rust 听写链路、现有 React 胶囊、一个能关闭/重开的普通窗口和托盘。先攻 GNOME，再用相同组件跑 KWin 和一个平铺合成器，最后补齐上表全部环境。编译和打包使用云端 runner；焦点、首帧、独占全屏与混合 DPI 用有真实图形会话、显示输出和麦克风的测试机验收。记录系统/合成器、引擎、扩展、输入法、目标应用版本，并确认 Wayland 表项的两个应用都实际使用 Wayland。项目现有 Linux CI 默认暂停，按标签启用的是构建验证；它不覆盖这些桌面结果。[现有 CI](../.github/workflows/ci.yml)

| 顺序 | 操作 | 通过依据 |
|---|---|---|
| 1 · 真实目标 | 打开外部编辑器，在「甲｜乙」中间放光标，旁边放第二个输入框；关闭并实际销毁主窗口，再通过全局热键开始听写。Win 可用记事本，macOS 用文本编辑，Linux 用该会话的原生 GTK/Qt 编辑器；每端再选一个浏览器输入框。 | 胶囊在正确屏幕及目标上方，透明、无框、无额外任务栏项。录像或合成器呈现证据显示正确首帧，音量和文字随真实录音变化，主窗没有被重新打开。 |
| 2 · 点击与焦点 | 在目标中连续输入标记字；依次点击胶囊按钮、展开样式/翻译菜单、选项、滚动、关闭，再从透明边缘、菜单外部和收起后的原位置点向下方控件。透明区测试用不会主动换输入焦点的计数控件。 | 按钮/下方控件各只收到自己的点击；首次点击即生效。用目标的连续 focus/blur 记录、键盘事件、系统活动窗口/辅助技术或 IM 上下文观测整个过程；目标没有瞬时失焦，标记字连续，菜单不截走普通键。只检查最后一次焦点不足以通过。 |
| 3 · 热键和落字 | 测原有按住说话/松开、切换、裸修饰、左右键、组合打断、重复、Esc、适用的 Fn/Shift 与现有菜单用键。完成真实识别，核对预测文本、实际目标全文、选区/光标、历史与剪贴板恢复。保留输入法预编辑/候选框的一轮。 | 原光标得到「甲＋听写结果＋乙」，只插入一次；第二个输入框无变化，取消无插入，热键字符不泄漏。切换/销毁目标的负例应返回既有失效或恢复状态；只写入剪贴板不能算通过。 |
| 4 · 连续两轮和故障 | A 已松键并在处理/播放退场时立即启动 B，故意延后 A 的完成、关闭、菜单回包。分别在录音期间隐藏、销毁主窗及结束 UI 子进程，再从托盘重开。 | B 首帧可见，旧回调和镜像不能误关 B 或作用于 B，A/B 的音量、文本、按钮和历史记录均属正确会话。重连补到最新状态，录音不中断，也不重复写回。 |
| 5 · 多屏、全屏、桌面 | 两块不同 DPI 的屏，分别把光标和目标应用放在不同屏；切主显示器/前台窗口屏/光标屏/所有屏镜像模式，跨屏、拔屏、切工作区，在普通全屏、macOS 全屏 Space、Windows 独占全屏等适用场景重复第 1–3 步。 | 位置、层级、首帧、按钮和透明区命中仍准确；不存在旧工作区幽灵窗口。对独占全屏或某桌面无法实现的项，保留实测失败或未验证结论。 |
| 6 · 正常编辑和退出 | 回主窗设置/Ask 实际使用输入法、Tab、读屏；用长翻译、长转录、大字和小窗核对实际画面。通过托盘在空闲和任务中分别显式退出，再启动一次。 | 普通窗口能得到键盘焦点，控件可见可操作，返回结果与反馈一致。退出遵循当前任务结束策略，释放麦克风、热键、托盘和子进程；重启可重新注册。保存关键画面、焦点轨迹、会话编号和目标前后文本，清理本轮临时数据。 |

**采用判据。** 所有承诺环境都走通“主窗已关 → 非激活胶囊 → 点击与穿透 → 原光标只写一次”，连续两轮无误关，再比较性能与维护成本。GNOME 扩展通过、Qt 的旧系统覆盖有可维护解、首帧和空闲成本可接受时，选 Qt WebEngine。若瓶颈是 Qt 的版本覆盖或体积，试原生 WebView；若瓶颈是标准 GNOME 的窗口权限，改换 Web 引擎无法消除它，保留“尚无满足全部需求的方案”的结论。
