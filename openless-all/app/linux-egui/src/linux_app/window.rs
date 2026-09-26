use super::*;

pub(super) struct WindowState {
    pub(super) should_be_open: bool,
    pub(super) error: Option<String>,
    pub(super) focus_requested: bool,
    pub(super) child: Option<std::process::Child>,
    pub(super) spawned_at: Option<std::time::Instant>,
    pub(super) last_snapshot_fingerprint: Option<u64>,
    pub(super) last_snapshot_at: std::time::Instant,
}

impl WindowState {
    pub(super) fn new(should_be_open: bool) -> Self {
        Self {
            should_be_open,
            error: None,
            focus_requested: should_be_open,
            child: None,
            spawned_at: None,
            last_snapshot_fingerprint: None,
            last_snapshot_at: std::time::Instant::now(),
        }
    }
}

impl Drop for WindowState {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            if child.try_wait().ok().flatten().is_none() {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
    }
}

/// 主窗口的最小内尺寸（UI 进程创建窗口时用它，和 `with_min_inner_size` 同源）。
pub(super) const MAIN_WINDOW_MIN_INNER_SIZE: egui::Vec2 = egui::vec2(960.0, 640.0);

/// MSAA sample count of the main window (`NativeOptions::multisampling`). The
/// settings backdrop draws the page offscreen through eframe's renderer, so its
/// colour target has to match the pipelines eframe built.
pub(super) const MAIN_MSAA_SAMPLES: u16 = 4;

/// 主窗口的初始尺寸。基准 = macOS（Tauri 的 main 窗口 1300×835）；
/// 旧的 Linux 专用窗口配置不作为依据。
pub(super) const MAIN_WINDOW_INNER_SIZE: [f32; 2] = [1300.0, 835.0];

/// 视图模型载荷指纹（FNV-1a 64）。够快，用来判断「要不要重发快照」：
/// 内容没变就不发，UI 慢的时候也不会被无意义的帧糊住。
pub(super) fn snapshot_fingerprint(payload: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in payload {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

impl OpenLessEguiApp {
    /// 与渲染无关的宿主心跳：原生事件、托盘命令、自动更新检查、泵心跳日志。
    ///
    /// 宿主循环（`run_host`）每 50ms 调它一次。窗口是独立进程，所以热键消费
    /// 与弹窗拉起完全不依赖「窗口是否在绘制」——窗口关掉、最小化、压根没开，
    /// 后台照样收键、照样把弹窗进程拉起来。
    pub(super) fn tick(&mut self, ctx: &egui::Context) {
        self.poll(ctx);
        self.drain_tray(ctx);
        self.log_pump_heartbeat(ctx);
    }

    /// 「显示主窗口」：宿主只记意图，由 `run_host` 拉起/抬起 UI 窗口进程。
    pub(super) fn request_main_window(&mut self) {
        self.window.should_be_open = true;
        self.window.focus_requested = true;
    }

    pub(super) fn focus_main_window(&mut self, bridge: &mut UiBridgeHost) {
        if self.window.focus_requested && bridge.is_connected() {
            bridge.send(HostToWindow::FocusMain);
            self.window.focus_requested = false;
            log::info!("[ui-host] focus request delivered; compositor decides activation");
        }
    }

    /// UI 窗口进程是否还活着（顺带回收已经退出的子进程）。
    pub(super) fn ui_window_alive(&mut self) -> bool {
        let Some(child) = self.window.child.as_mut() else {
            return false;
        };
        match child.try_wait() {
            Ok(None) => true,
            Ok(Some(status)) => {
                if !status.success() {
                    self.window.error = Some(format!("UI window process failed: {status}"));
                }
                log::info!("[ui-host] UI window process exited ({status}); host keeps running");
                self.window.child = None;
                false
            }
            Err(error) => {
                self.window.error = Some(format!("UI window process wait failed: {error}"));
                log::warn!("[ui-host] UI window process wait failed: {error}");
                self.window.child = None;
                false
            }
        }
    }

    /// 拉起 UI 窗口进程。
    ///
    /// 时序：调用方保证宿主已经 bind 好桥 socket（UI 进程连不上就直接报错退出，
    /// 不会自己抢单实例锁或打开数据目录）。
    pub(super) fn spawn_ui_window(&mut self, socket: &std::path::Path) -> Result<(), String> {
        let executable = std::env::current_exe().map_err(|error| error.to_string())?;
        let mut command = std::process::Command::new(executable);
        command
            .arg(UI_CLIENT_FLAG)
            .arg(UI_SOCKET_FLAG)
            .arg(socket)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            // 让 panic / winit 警告走宿主自己的 stderr（终端或 journal），
            // 否则「窗口没起来」会变成一条无声的失败。
            .stderr(std::process::Stdio::inherit());
        let child = command.spawn().map_err(|error| error.to_string())?;
        log::info!(
            "[ui-host] spawned UI window process pid={} socket={}",
            child.id(),
            socket.display()
        );
        self.window.child = Some(child);
        self.window.spawned_at = Some(std::time::Instant::now());
        Ok(())
    }

    /// 窗口当前是否需要一个 UI 进程：用户想开着，而且现在没有活着的窗口。
    /// 刚拉起的 800ms 内不重复拉起，避免连点托盘菜单拉出两个窗口。
    pub(super) fn should_spawn_ui_window(&mut self) -> bool {
        let alive = self.ui_window_alive();
        if !self.window.should_be_open || alive || self.window.error.is_some() {
            return false;
        }
        if let Some(spawned_at) = self.window.spawned_at {
            if spawned_at.elapsed() < Duration::from_millis(800) {
                return false;
            }
        }
        true
    }

    /// 处理 UI 进程发来的消息（含断连语义）。
    ///
    /// 时序：`Bye`/断开只把窗口标记为关闭，**不动**后端、会话与弹窗；
    /// 没有托盘时则连宿主一起退出 —— 否则用户再也找不到这个进程。
    pub(super) fn apply_window_messages(
        &mut self,
        messages: Vec<WindowToHost>,
        tray_available: bool,
    ) {
        for message in messages {
            match message {
                WindowToHost::Hello { version } => {
                    if version != UI_BRIDGE_VERSION {
                        log::warn!(
                                "[ui-host] UI window speaks protocol {version}, host speaks {UI_BRIDGE_VERSION}"
                            );
                    } else {
                        log::info!("[ui-host] UI window handshake ok (protocol {version})");
                    }
                    // 窗口进程刚起来（可能是重启）：热键配置必须无条件重发一份，
                    // 否则新窗口拿不到绑定，它自己就没法匹配本地热键。
                    self.hotkeys_sent = None;
                    self.window.last_snapshot_fingerprint = None;
                }
                WindowToHost::Action { sequence, action } => {
                    log::debug!("[ui-host] UI action #{sequence}: {action:?}");
                    self.pending_ui_actions.push(*action);
                }
                WindowToHost::Hotkey { sequence, edge } => {
                    log::info!("[hotkey] local edge from the UI window #{sequence}: {edge:?}");
                    self.pending_local_hotkeys
                        .push((std::time::Instant::now(), edge));
                }
                WindowToHost::Ping { sequence } => {
                    self.pending_ui_pongs.push(sequence);
                }
                WindowToHost::Bye => {
                    log::info!("[ui-host] UI window said goodbye; host keeps running");
                    self.window.should_be_open = false;
                }
            }
        }
        if !self.window.should_be_open && !tray_available {
            // 没有托盘就没有重新打开的入口，窗口退出等于应用退出。
            log::info!("[ui-host] no tray to reopen the window; exiting with it");
            self.exit_requested = true;
        }
    }

    /// 把当前视图模型发给 UI 进程：内容变过、或距上次超过 2s（保活）才发。
    pub(super) fn publish_view_model(&mut self, bridge: &mut UiBridgeHost) {
        if !bridge.is_connected() {
            // UI 不在：清掉指纹，等它回来时无条件发一份完整快照。
            self.window.last_snapshot_fingerprint = None;
            return;
        }
        let payload = match serde_json::to_vec(&self.frontend_vm) {
            Ok(payload) => payload,
            Err(error) => {
                log::warn!("[ui-host] view model serialization failed: {error}");
                return;
            }
        };
        let fingerprint = snapshot_fingerprint(&payload);
        let keepalive = self.window.last_snapshot_at.elapsed() >= Duration::from_secs(2);
        if Some(fingerprint) == self.window.last_snapshot_fingerprint && !keepalive {
            return;
        }
        self.window.last_snapshot_fingerprint = Some(fingerprint);
        self.window.last_snapshot_at = std::time::Instant::now();
        bridge.send_snapshot_encoded(&payload);
    }
}

/// UI 窗口进程入口：只渲染。
///
/// 它不构造 Core 后端、不打开数据目录、不抢单实例锁、不注册托盘与热键 ——
/// 关掉它等于「关掉一个窗口」，宿主与所有后台能力原地不动。
pub(super) fn vulkan_options(mut options: eframe::NativeOptions) -> eframe::NativeOptions {
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup {
        setup.instance_descriptor.backends = eframe::egui_wgpu::wgpu::Backends::VULKAN;
    }
    options
}

pub(super) fn run_ui_client(socket: std::path::PathBuf) -> Result<(), String> {
    // UI 进程不复用宿主的日志器对象，但写到同一个文件里，
    // 排查「窗口进程怎么没了」时两端日志在同一处。
    if let Ok(data_dir) = openless_data_dir() {
        if let Err(error) = openless_linux_egui::init_file_logger(&data_dir) {
            eprintln!("OpenLess UI window logger unavailable: {error}");
        }
    }
    let client = UiBridgeClient::connect(&socket)?;
    log::info!(
        "[ui-client] connected to the host bridge at {}",
        socket.display()
    );
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("OpenLess")
            .with_inner_size(MAIN_WINDOW_INNER_SIZE)
            .with_min_inner_size(MAIN_WINDOW_MIN_INNER_SIZE)
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(true)
            .with_visible(true),
        renderer: eframe::Renderer::Wgpu,
        multisampling: MAIN_MSAA_SAMPLES,
        ..Default::default()
    };
    let options = vulkan_options(options);
    eframe::run_native(
        "OpenLess",
        options,
        Box::new(move |cc| {
            theme::install(&cc.egui_ctx);
            Ok(Box::new(UiClientApp::new(
                client,
                cc.wgpu_render_state.as_ref(),
            )))
        }),
    )
    .map_err(|error| error.to_string())
}

/// 诊断开关：`OPENLESS_UI_DEBUG=1` 时 UI 进程把指针点击与动作记进日志。
/// 用来分辨「按钮没反应」是没收到指针事件，还是动作没能送到宿主。
pub(super) fn ui_debug_enabled() -> bool {
    std::env::var("OPENLESS_UI_DEBUG").is_ok_and(|value| value == "1")
}

/// 是否采纳这一份快照：序号必须**严格递增**（重复、乱序、回退统统丢弃），
/// 否则 UI 会把新状态画成旧状态。
pub(super) fn snapshot_supersedes(last_sequence: u64, sequence: u64) -> bool {
    sequence > last_sequence
}

/// 三方合并：`base` = 宿主上一份快照，`local` = 本地（用户可能正在输入的）
/// 视图模型，`incoming` = 宿主新快照。
///
/// 用户可见文本全都直接绑在视图模型的字段上（`TextEdit::singleline(&mut
/// vm.…)×`），而宿主每 2s、以及任何状态变化时都会推一份完整快照；UI 侧原先
/// 是整份替换，于是「打好字还没提交」的输入会被下一份快照抹掉 —— 用户看到的
/// 就是「输入一秒后文字自己消失」。这里按字段判断：宿主相对上一份快照**改过**
/// 的字段以宿主为准（打开编辑器时 hydrate、提交后回写、宿主侧列表刷新都走这条），
/// **没改过**的字段保留本地值（用户正在输入的内容）。
pub(super) fn merge_local_edits(
    local: &serde_json::Value,
    base: &serde_json::Value,
    incoming: &serde_json::Value,
) -> serde_json::Value {
    if let serde_json::Value::Object(incoming_fields) = incoming {
        let mut merged = serde_json::Map::new();
        for (key, incoming_value) in incoming_fields {
            let base_value = base.get(key).unwrap_or(&serde_json::Value::Null);
            let value = match local.get(key) {
                Some(local_value) => merge_local_edits(local_value, base_value, incoming_value),
                None => incoming_value.clone(),
            };
            merged.insert(key.clone(), value);
        }
        serde_json::Value::Object(merged)
    } else if incoming == base {
        // 宿主没动这个字段：本地值（可能是没提交的输入）留下。
        local.clone()
    } else {
        incoming.clone()
    }
}

/// UI 进程侧的 eframe 应用：收快照 → 渲染 → 把动作发回宿主。
pub(super) struct UiClientApp {
    client: UiBridgeClient,
    connecting_since: std::time::Instant,
    connection_error: Option<String>,
    pub(super) view_model: FrontendViewModel,
    /// 已采纳的最大快照序号。
    last_sequence: u64,
    /// 宿主上一份快照的 JSON：用来分辨「宿主改了这个字段」和「用户还没提交的
    /// 本地输入」。
    last_adopted: Option<serde_json::Value>,
    /// 已发出的动作序号（宿主可据此看出重复或丢失）。
    action_sequence: u64,
    ping_sequence: u64,
    ping_sent_at: Option<std::time::Instant>,
    last_ping_at: std::time::Instant,
    latency_samples: Vec<u128>,
    exited: bool,
    /// 宿主下发的本地热键配置（窗口有焦点时插件收不到按键）。
    hotkeys: Option<openless_core::HotkeyRuntimeTarget>,
    hotkey_matcher: crate::ui::local_hotkeys::LocalHotkeyMatcher,
    /// 本地热键边沿的发送序号（与动作序号分开，便于日志区分）。
    hotkey_sequence: u64,
    /// 设置页模糊背板；只有拿到 wgpu 渲染状态（正常 GUI 进程）时存在。
    backdrop: Option<crate::ui::backdrop::BackdropBlur>,
}

impl UiClientApp {
    pub(super) fn new(
        client: UiBridgeClient,
        render_state: Option<&eframe::egui_wgpu::RenderState>,
    ) -> Self {
        Self {
            client,
            connecting_since: std::time::Instant::now(),
            connection_error: None,
            // 设置页的磨砂背板：把遮罩下方的页面离屏重绘后做真实高斯模糊。
            backdrop: render_state.map(|state| {
                crate::ui::backdrop::BackdropBlur::new(state, MAIN_MSAA_SAMPLES.into())
            }),
            view_model: FrontendViewModel::default(),
            last_sequence: 0,
            last_adopted: None,
            action_sequence: 0,
            ping_sequence: 0,
            ping_sent_at: None,
            last_ping_at: std::time::Instant::now(),
            latency_samples: Vec::new(),
            exited: false,
            hotkeys: None,
            hotkey_matcher: crate::ui::local_hotkeys::LocalHotkeyMatcher::default(),
            hotkey_sequence: 0,
        }
    }

    /// 采纳一份快照，但保留用户还没提交、而宿主也没有改动的输入。
    pub(super) fn adopt_snapshot(&mut self, sequence: u64, incoming: FrontendViewModel) {
        self.last_sequence = sequence;
        let incoming_json = serde_json::to_value(&incoming).ok();
        let adopted = match (&self.last_adopted, &incoming_json) {
            (Some(base), Some(incoming_json)) => serde_json::to_value(&self.view_model)
                .ok()
                .and_then(|local| {
                    serde_json::from_value(merge_local_edits(&local, base, incoming_json)).ok()
                })
                .unwrap_or(incoming),
            // 第一份快照没有可比对的基准：整份采纳。
            _ => incoming,
        };
        self.view_model = adopted;
        self.last_adopted = incoming_json;
    }

    /// 收宿主的帧。快照按序号采纳；`Shutdown` 与断连都表示「宿主走了」，
    /// 此时 UI 必须自己退出（没有宿主就没有数据可渲染）。
    pub(super) fn drain_host(&mut self, ctx: &egui::Context) {
        loop {
            match self.client.try_recv() {
                Ok(HostToWindow::Ready { version }) => {
                    log::info!("[ui-client] host ready (protocol {version})");
                }
                Ok(HostToWindow::FocusMain) => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                Ok(HostToWindow::Rejected { reason }) => {
                    self.connection_error = Some(reason);
                    self.client.shutdown();
                    break;
                }
                Ok(HostToWindow::Hotkeys { bindings, .. }) => {
                    // 窗口有焦点时 fcitx5 收不到按键，本地匹配全靠这份配置。
                    log::info!("[ui-client] local hotkey bindings received");
                    self.hotkeys = Some(*bindings);
                }
                Ok(HostToWindow::Snapshot {
                    sequence,
                    view_model,
                }) => {
                    if snapshot_supersedes(self.last_sequence, sequence) {
                        self.adopt_snapshot(sequence, *view_model);
                    } else {
                        log::debug!("[ui-client] dropped stale snapshot #{sequence}");
                    }
                }
                Ok(HostToWindow::Pong { sequence }) => {
                    if let Some(sent_at) = self.ping_sent_at.take() {
                        let rtt = sent_at.elapsed().as_millis();
                        self.latency_samples.push(rtt);
                        if self.latency_samples.len() >= 20 {
                            let count = self.latency_samples.len();
                            let max = self.latency_samples.iter().copied().max().unwrap_or(0);
                            let sum: u128 = self.latency_samples.iter().sum();
                            log::info!(
                                    "[ui-client] ipc round-trip avg={}ms max={max}ms over {count} probes (last #{sequence})",
                                    sum / count as u128
                                );
                            self.latency_samples.clear();
                        }
                    }
                }
                Ok(HostToWindow::Shutdown) => {
                    log::info!("[ui-client] host asked to shut down; closing the window");
                    self.exited = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    log::warn!("[ui-client] host connection lost; closing the window");
                    self.exited = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    break;
                }
            }
        }
    }

    /// 本窗口内命中的热键作为边沿报给宿主。
    ///
    /// 只读 `InputState`（不消费事件），命中才发一帧；正在录制快捷键时跳过，
    /// 否则用户在设置里录「Alt+A」会顺手触发一次听写。
    pub(super) fn poll_local_hotkeys(&mut self, ctx: &egui::Context) {
        let Some(bindings) = self.hotkeys.as_ref() else {
            return;
        };
        if self.view_model.shortcut_recording.is_some() {
            return;
        }
        let Some(edge) = self.hotkey_matcher.poll(ctx, bindings) else {
            return;
        };
        self.hotkey_sequence += 1;
        if let Err(error) = self.client.send(WindowToHost::Hotkey {
            sequence: self.hotkey_sequence,
            edge,
        }) {
            log::warn!("[ui-client] cannot forward a local hotkey to the host: {error}");
        }
    }

    pub(super) fn ping_if_due(&mut self) {
        if self.last_ping_at.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.last_ping_at = std::time::Instant::now();
        self.ping_sequence += 1;
        self.ping_sent_at = Some(std::time::Instant::now());
        let _ = self.client.send(WindowToHost::Ping {
            sequence: self.ping_sequence,
        });
    }

    /// 关窗 = 本进程退出。宿主仍在，会话、录音、弹窗都不受影响。
    pub(super) fn request_exit(&mut self, ctx: &egui::Context, reason: &str) {
        if self.exited {
            return;
        }
        log::info!("[ui-client] {reason}: exiting the window process (host keeps running)");
        self.exited = true;
        let _ = self.client.send(WindowToHost::Bye);
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    /// 把渲染层产生的动作分发出去：窗口控制就地处理，其余发给宿主。
    pub(super) fn dispatch(
        &mut self,
        actions: Vec<frontend::view_model::FrontendAction>,
        ctx: &egui::Context,
    ) {
        for action in actions {
            match action {
                frontend::view_model::FrontendAction::WindowClose => {
                    self.request_exit(ctx, "close button");
                }
                frontend::view_model::FrontendAction::WindowMinimize => {
                    // 最小化在 Wayland 上是单向门（winit 明确拒绝取消最小化），
                    // 而宿主已经接管热键与弹窗，所以按「关窗回托盘」处理：
                    // 窗口进程退出，任务栏条目消失，托盘随时能再开一个。
                    self.request_exit(ctx, "minimize button");
                }
                frontend::view_model::FrontendAction::WindowMaximize => {
                    let maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                }
                other => {
                    self.action_sequence += 1;
                    if let Err(error) = self.client.send(WindowToHost::Action {
                        sequence: self.action_sequence,
                        action: Box::new(other),
                    }) {
                        log::warn!("[ui-client] cannot forward action to the host: {error}");
                    }
                }
            }
        }
    }
}

impl Drop for UiClientApp {
    fn drop(&mut self) {
        // 关窗即退出：告别帧让宿主立刻作废窗口句柄，收尾读写线程
        // 以免宿主一直等到 EOF。
        let _ = self.client.send(WindowToHost::Bye);
        self.client.shutdown();
    }
}

impl eframe::App for UiClientApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Color32::TRANSPARENT.to_normalized_gamma_f32()
    }

    /// 合成器拖窗（标题栏按下即交给它）会吃掉释放事件、并且不在拖动期间发 motion，
    /// egui 的按压/拖拽状态与指针坐标都会留在原地——页面因此滚不动，直到用户点一下。
    /// `raw_input_hook` 是唯一能赶在这一次 pass 之前修补事件的地方。
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        frontend::layout::route_pointer_before_pass(ctx, raw_input);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.connection_error.is_none() {
            self.drain_host(&ctx);
        }
        if !self.client.is_ready() || self.connection_error.is_some() {
            if self.connection_error.is_none()
                && self.connecting_since.elapsed() >= bridge::UI_CLIENT_CONNECT_TIMEOUT
            {
                self.connection_error =
                    Some("UI handshake timed out; restart the complete application".into());
                self.client.shutdown();
            }
            ui.label(
                self.connection_error
                    .as_deref()
                    .unwrap_or_else(|| tr_l10n(self.view_model.lang, "startup.connecting")),
            );
            if ui
                .button(tr_l10n(self.view_model.lang, "common.close"))
                .clicked()
            {
                self.request_exit(&ctx, "connection dialog close");
            }
            ctx.request_repaint_after(Duration::from_millis(50));
            return;
        }
        self.poll_local_hotkeys(&ctx);
        theme::apply_visuals(&ctx, self.view_model.theme_mode);
        if ctx.input(|input| input.viewport().close_requested()) {
            self.request_exit(&ctx, "window manager close request");
        }
        if self.exited {
            return;
        }
        if ui_debug_enabled() {
            let (pointer, clicked) =
                ctx.input(|input| (input.pointer.interact_pos(), input.pointer.any_click()));
            if clicked {
                log::info!("[ui-client] pointer click at {pointer:?}");
            }
        }
        // 所有前端弹窗共用实时磨砂背板：先按当前窗口尺寸准备离屏目标，
        // 把纹理 id 发布给遮罩；页面在这一帧稍后离屏重绘并模糊（都在 eframe 绘制之前完成，
        // 所以遮罩采到的是当前帧的像素，不需要任何“等一帧截图”的补丁）。
        {
            let open = self.view_model.settings_open
                || self.view_model.style_editor_open
                || self.view_model.marketplace_selected.is_some()
                || self.view_model.marketplace_mine_open
                || self.view_model.marketplace_upload_open
                || self.view_model.marketplace_confirm_withdraw.is_some()
                || self.view_model.history_confirm.is_some()
                || (self.view_model.active_page == frontend::view_model::Page::Vocab
                    && frontend::vocab::new_word_overlay_open(&ctx));
            let texture = self
                .backdrop
                .as_mut()
                .and_then(|backdrop| backdrop.prepare(&ctx, open));
            crate::ui::backdrop::publish(&ctx, texture);
        }
        let mut actions = Vec::new();
        frontend::render(&ctx, &mut self.view_model, &mut actions);
        let modal_open = self.view_model.settings_open
            || self.view_model.style_editor_open
            || self.view_model.marketplace_selected.is_some()
            || self.view_model.marketplace_mine_open
            || self.view_model.marketplace_upload_open
            || self.view_model.marketplace_confirm_withdraw.is_some()
            || self.view_model.history_confirm.is_some()
            || (self.view_model.active_page == frontend::view_model::Page::Vocab
                && frontend::vocab::new_word_overlay_open(&ctx));
        if modal_open {
            if let Some(backdrop) = self.backdrop.as_mut() {
                backdrop.render_page(&ctx);
            }
        }
        if ui_debug_enabled() && !actions.is_empty() {
            log::info!("[ui-client] actions from the renderer: {actions:?}");
        }
        self.dispatch(actions, &ctx);
        self.ping_if_due();
        // Shortcut capture is sampled from egui input events; refresh its
        // inline recorder promptly so capture/cancel closes in the same
        // perceptual beat as the key press. Idle settings keep the 30ms cap.
        let repaint_ms = if self.view_model.shortcut_recording.is_some() {
            12
        } else {
            30
        };
        ctx.request_repaint_after(Duration::from_millis(repaint_ms));
    }
}

struct StartupErrorApp {
    error: String,
    lang: Lang,
    broker: Arc<SingleInstanceBroker>,
}

impl eframe::App for StartupErrorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let mut focus = false;
        self.broker.drain(|_| focus = true);
        if focus {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        ui.heading(tr_l10n(self.lang, "status.startup_failed"));
        ui.add_space(12.0);
        ui.label(&self.error);
        ui.add_space(12.0);
        ui.label(tr_l10n(self.lang, "startup.fcitx_help"));
        if ui.button(tr_l10n(self.lang, "common.close")).clicked() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ui.ctx().request_repaint_after(Duration::from_millis(100));
    }
}

pub(super) fn show_startup_error(error: &str, broker: Arc<SingleInstanceBroker>) {
    eprintln!("OpenLess startup failed: {error}");
    if ["DISPLAY", "WAYLAND_DISPLAY"]
        .iter()
        .all(|name| std::env::var(name).unwrap_or_default().is_empty())
    {
        return;
    }
    let lang = load_locale_pref().resolve();
    let error = error.to_string();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(tr_l10n(lang, "status.startup_failed"))
            .with_inner_size([560.0, 300.0]),
        ..Default::default()
    };
    if let Err(failure) = eframe::run_native(
        "OpenLess",
        options,
        Box::new(move |cc| {
            theme::install(&cc.egui_ctx);
            Ok(Box::new(StartupErrorApp {
                error,
                lang,
                broker,
            }))
        }),
    ) {
        eprintln!("OpenLess startup error window failed: {failure}");
    }
}
