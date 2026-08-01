#![cfg_attr(target_os = "linux", allow(dead_code, unused_variables))]
//! Linux fcitx5 插件 DBus 客户端。
//!
//! 封装对 `org.fcitx.Fcitx.OpenLess1` 接口的调用，
//! 提供文字提交（替代 enigo XTest）和热键设置功能。
//!
//! 所有函数会静默返回 `None` 如果 fcitx5 / 插件不可用，
//! 调用方应当降级到原有方案（clipboard / enigo）。

use std::time::Duration;

use dbus::blocking::BlockingSender;

const DEST: &str = "org.fcitx.Fcitx5";
const PATH: &str = "/openless";
const IFACE: &str = "org.fcitx.Fcitx.OpenLess1";
const TIMEOUT: Duration = Duration::from_secs(3);

/// 通过 fcitx5 插件向当前焦点输入上下文提交文字。
///
/// 返回 `Ok(())` 表示文字已提交，`Err` 表示调用失败（插件未加载 / DBus 不通等）。
pub fn commit_text(text: &str) -> Result<(), String> {
    let conn =
        dbus::blocking::Connection::new_session().map_err(|e| format!("dbus session: {e}"))?;
    let msg = dbus::Message::new_method_call(DEST, PATH, IFACE, "CommitText")
        .map_err(|e| format!("build msg: {e}"))?
        .append1(text);
    conn.send_with_reply_and_block(msg, TIMEOUT)
        .map_err(|e| format!("CommitText: {e}"))?;
    Ok(())
}

/// 通过 fcitx5 插件设置听写触发快捷键。
///
/// `keys` 为 Key::parse 格式的字符串数组，例如 `["Control+space"]`。
pub fn set_hotkey(keys: &[&str]) -> Result<(), String> {
    let conn =
        dbus::blocking::Connection::new_session().map_err(|e| format!("dbus session: {e}"))?;
    let list: Vec<String> = keys.iter().map(|s| s.to_string()).collect();
    let msg = dbus::Message::new_method_call(DEST, PATH, IFACE, "SetHotkey")
        .map_err(|e| format!("build msg: {e}"))?
        .append1(list);
    conn.send_with_reply_and_block(msg, TIMEOUT)
        .map_err(|e| format!("SetHotkey: {e}"))?;
    Ok(())
}

/// 通过 fcitx5 插件直接设置 sym + states 作为触发键。
pub fn set_hotkey_raw(sym: u32, states: u32) -> Result<(), String> {
    let conn =
        dbus::blocking::Connection::new_session().map_err(|e| format!("dbus session: {e}"))?;
    let msg = dbus::Message::new_method_call(DEST, PATH, IFACE, "SetHotkeyRaw")
        .map_err(|e| format!("build msg: {e}"))?
        .append2(sym, states);
    conn.send_with_reply_and_block(msg, TIMEOUT)
        .map_err(|e| format!("SetHotkeyRaw: {e}"))?;
    Ok(())
}

/// 通过 fcitx5 插件设置 QA 面板快捷键 sym + states。
pub fn set_qa_hotkey_raw(sym: u32, states: u32) -> Result<(), String> {
    let conn =
        dbus::blocking::Connection::new_session().map_err(|e| format!("dbus session: {e}"))?;
    let msg = dbus::Message::new_method_call(DEST, PATH, IFACE, "SetQaHotkeyRaw")
        .map_err(|e| format!("build msg: {e}"))?
        .append2(sym, states);
    conn.send_with_reply_and_block(msg, TIMEOUT)
        .map_err(|e| format!("SetQaHotkeyRaw: {e}"))?;
    Ok(())
}

/// 通过 fcitx5 插件设置翻译模式修饰键 sym + states。
pub fn set_translation_hotkey_raw(sym: u32, states: u32) -> Result<(), String> {
    let conn =
        dbus::blocking::Connection::new_session().map_err(|e| format!("dbus session: {e}"))?;
    let msg = dbus::Message::new_method_call(DEST, PATH, IFACE, "SetTranslationHotkeyRaw")
        .map_err(|e| format!("build msg: {e}"))?
        .append2(sym, states);
    conn.send_with_reply_and_block(msg, TIMEOUT)
        .map_err(|e| format!("SetTranslationHotkeyRaw: {e}"))?;
    Ok(())
}

/// X11 keysym 值（用于 SetHotkeyRaw / SetQaHotkeyRaw / SetTranslationHotkeyRaw，
/// 绕过 Key::parse 的修饰键限制）。
const KEYSYM_CONTROL_R: u32 = 0xffe4;
const KEYSYM_CONTROL_L: u32 = 0xffe3;
const KEYSYM_ALT_R: u32 = 0xffea;
const KEYSYM_ALT_L: u32 = 0xffe9;
const KEYSYM_SUPER_R: u32 = 0xffec;
const KEYSYM_SUPER_L: u32 = 0xffeb;
const KEYSYM_SHIFT_R: u32 = 0xffe2;
const KEYSYM_SHIFT_L: u32 = 0xffe1;

/// 将 HotkeyTrigger 转换为 X11 keysym。
fn trigger_to_keysym(trigger: crate::types::HotkeyTrigger) -> u32 {
    match trigger {
        crate::types::HotkeyTrigger::RightControl => KEYSYM_CONTROL_R,
        crate::types::HotkeyTrigger::LeftControl => KEYSYM_CONTROL_L,
        crate::types::HotkeyTrigger::RightOption | crate::types::HotkeyTrigger::RightAlt => {
            KEYSYM_ALT_R
        }
        crate::types::HotkeyTrigger::LeftOption => KEYSYM_ALT_L,
        crate::types::HotkeyTrigger::RightCommand => KEYSYM_SUPER_R,
        crate::types::HotkeyTrigger::LeftCommand => KEYSYM_SUPER_L,
        crate::types::HotkeyTrigger::LeftShift => KEYSYM_SHIFT_L,
        crate::types::HotkeyTrigger::RightShift => KEYSYM_SHIFT_R,
        crate::types::HotkeyTrigger::Fn => KEYSYM_CONTROL_R,
        crate::types::HotkeyTrigger::MediaPlayPause => unreachable!("Windows-only"),
        crate::types::HotkeyTrigger::Custom => unreachable!(),
    }
}

fn trigger_name(trigger: crate::types::HotkeyTrigger) -> &'static str {
    match trigger {
        crate::types::HotkeyTrigger::RightControl => "Control_R",
        crate::types::HotkeyTrigger::LeftControl => "Control_L",
        crate::types::HotkeyTrigger::RightOption | crate::types::HotkeyTrigger::RightAlt => "Alt_R",
        crate::types::HotkeyTrigger::LeftOption => "Alt_L",
        crate::types::HotkeyTrigger::RightCommand => "Super_R",
        crate::types::HotkeyTrigger::LeftCommand => "Super_L",
        crate::types::HotkeyTrigger::LeftShift => "Shift_L",
        crate::types::HotkeyTrigger::RightShift => "Shift_R",
        crate::types::HotkeyTrigger::Fn => "Control_R",
        crate::types::HotkeyTrigger::MediaPlayPause => unreachable!("Windows-only"),
        crate::types::HotkeyTrigger::Custom => unreachable!(),
    }
}

/// 将 OpenLess 的主听写热键绑定同步到 fcitx5 插件。
pub fn sync_binding_to_plugin(binding: &crate::types::HotkeyBinding) {
    if binding.trigger == crate::types::HotkeyTrigger::Custom
        || binding.trigger == crate::types::HotkeyTrigger::MediaPlayPause
    {
        return;
    }
    let sym = trigger_to_keysym(binding.trigger);
    let name = trigger_name(binding.trigger);
    match set_hotkey_raw(sym, 0) {
        Ok(()) => log::info!("[fcitx] Synced hotkey {name} (sym={sym}) to plugin via SetHotkeyRaw"),
        Err(e) => log::warn!("[fcitx] Failed to sync hotkey to plugin: {e}"),
    }
}

/// 将 ShortcutBinding 转换为 fcitx5 Key::parse 格式的字符串。
///
/// 例如 `modifiers: ["Ctrl", "Alt"], primary: "d"` → `"Control+Alt+d"`。
pub fn binding_to_fcitx_key_string(binding: &crate::types::ShortcutBinding) -> String {
    let mut parts: Vec<String> = Vec::new();
    for m in &binding.modifiers {
        let lower = m.to_lowercase();
        let normalized = match lower.as_str() {
            "ctrl" | "control" => "Control",
            "alt" | "option" | "opt" => "Alt",
            "shift" => "Shift",
            "super" | "meta" | "cmd" | "win" | "command" => "Super",
            other => other,
        };
        if !parts.contains(&normalized.to_string()) {
            parts.push(normalized.to_string());
        }
    }
    // 主键：取小写，去掉 "Key" 前缀（如 "KeyD" → "d"）
    let primary = binding.primary.trim();
    let primary = if let Some(stripped) = primary.strip_prefix("Key") {
        stripped.to_lowercase()
    } else {
        primary.to_lowercase()
    };
    if primary.is_empty() {
        return String::new();
    }
    if parts.is_empty() {
        primary
    } else {
        format!("{}+{}", parts.join("+"), primary)
    }
}

/// 通过 fcitx5 插件的 SetCustomDictationTrigger 方法设置自定义组合键。
pub fn set_custom_dictation_trigger(key_string: &str) -> Result<(), String> {
    let conn =
        dbus::blocking::Connection::new_session().map_err(|e| format!("dbus session: {e}"))?;
    let msg = dbus::Message::new_method_call(DEST, PATH, IFACE, "SetCustomDictationTrigger")
        .map_err(|e| format!("build msg: {e}"))?
        .append1(key_string);
    conn.send_with_reply_and_block(msg, TIMEOUT)
        .map_err(|e| format!("SetCustomDictationTrigger: {e}"))?;
    Ok(())
}

/// 将 QA 面板快捷键同步到 fcitx5 插件。
pub fn sync_qa_binding(trigger: Option<crate::types::HotkeyTrigger>) {
    let Some(trigger) = trigger else {
        // 无 QA 快捷键时清空插件端配置
        let _ = set_qa_hotkey_raw(0, 0);
        return;
    };
    if trigger == crate::types::HotkeyTrigger::MediaPlayPause {
        return;
    }
    let sym = trigger_to_keysym(trigger);
    let name = trigger_name(trigger);
    match set_qa_hotkey_raw(sym, 0) {
        Ok(()) => {
            log::info!("[fcitx] Synced QA hotkey {name} (sym={sym}) to plugin via SetQaHotkeyRaw")
        }
        Err(e) => log::warn!("[fcitx] Failed to sync QA hotkey to plugin: {e}"),
    }
}

/// 将翻译模式快捷键同步到 fcitx5 插件。
pub fn sync_translation_binding(trigger: Option<crate::types::HotkeyTrigger>) {
    let Some(trigger) = trigger else {
        let _ = set_translation_hotkey_raw(0, 0);
        return;
    };
    if trigger == crate::types::HotkeyTrigger::MediaPlayPause {
        return;
    }
    let sym = trigger_to_keysym(trigger);
    let name = trigger_name(trigger);
    match set_translation_hotkey_raw(sym, 0) {
        Ok(()) => log::info!("[fcitx] Synced translation hotkey {name} (sym={sym}) to plugin via SetTranslationHotkeyRaw"),
        Err(e) => log::warn!("[fcitx] Failed to sync translation hotkey to plugin: {e}"),
    }
}

/// 通过 fcitx5 插件在候选词列表下方显示状态文本（不干扰输入法预编辑）。
pub fn set_aux_down(text: &str) -> Result<(), String> {
    let conn =
        dbus::blocking::Connection::new_session().map_err(|e| format!("dbus session: {e}"))?;
    let msg = dbus::Message::new_method_call(DEST, PATH, IFACE, "SetAuxDown")
        .map_err(|e| format!("build msg: {e}"))?
        .append1(text);
    conn.send_with_reply_and_block(msg, TIMEOUT)
        .map_err(|e| format!("SetAuxDown: {e}"))?;
    Ok(())
}

/// 清除 fcitx5 插件候选词列表下方状态文本。
pub fn clear_aux_down() -> Result<(), String> {
    let conn =
        dbus::blocking::Connection::new_session().map_err(|e| format!("dbus session: {e}"))?;
    let msg = dbus::Message::new_method_call(DEST, PATH, IFACE, "ClearAuxDown")
        .map_err(|e| format!("build msg: {e}"))?;
    conn.send_with_reply_and_block(msg, TIMEOUT)
        .map_err(|e| format!("ClearAuxDown: {e}"))?;
    Ok(())
}

/// 快速检查 fcitx5 OpenLess 插件是否可用（DBus 对象存在）。
pub fn available() -> bool {
    let conn = match dbus::blocking::Connection::new_session() {
        Ok(c) => c,
        Err(_) => return false,
    };
    let msg = match dbus::Message::new_method_call(DEST, PATH, "org.freedesktop.DBus.Peer", "Ping")
    {
        Ok(m) => m,
        Err(_) => return false,
    };
    conn.send_with_reply_and_block(msg, TIMEOUT).is_ok()
}

/// 启动 fcitx5 DictationKeyEvent 信号监听线程。
///
/// 当 fcitx5 OpenLess 插件检测到配置的听写热键被按下或松开时，
/// 发出 `DictationKeyEvent(uub)` DBus 信号（sym, states, isPress）。
/// 本函数将此信号转发为 `HotkeyEvent::Pressed` / `Released` 到协调器事件通道。
///
/// 后台线程在 `tx` 全部 drop（协调器关闭）或 DBus 连接断开时自动退出。
///
/// 如果 fcitx5 尚未启动，线程会每 3 秒重试同步热键绑定，直到 fcitx5 可用。
/// 同时监听 `NameOwnerChanged` 信号以在 fcitx5 重启后重新同步。
#[cfg(target_os = "linux")]
pub fn start_dictation_signal_listener(
    tx: std::sync::mpsc::Sender<crate::hotkey::HotkeyEvent>,
    combo_tx: std::sync::mpsc::Sender<u64>,
    binding: crate::types::HotkeyBinding,
    qa_trigger: Option<crate::types::HotkeyTrigger>,
    translation_trigger: Option<crate::types::HotkeyTrigger>,
    custom_trigger_key: Option<String>,
) {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    std::thread::Builder::new()
        .name("openless-fcitx-signal".into())
        .spawn(move || {
            let conn = match dbus::blocking::SyncConnection::new_session() {
                Ok(c) => c,
                Err(e) => {
                    log::warn!("[fcitx-hotkey] DBus session failed: {e}");
                    return;
                }
            };

            // 同时监听所有三个 OpenLess 信号
            let rule = match dbus::message::MatchRule::parse(
                "type='signal',\
                 interface='org.fcitx.Fcitx.OpenLess1'",
            ) {
                Ok(r) => r,
                Err(e) => {
                    log::warn!("[fcitx-hotkey] Invalid match rule: {e}");
                    return;
                }
            };

            let tx2 = tx.clone();
            let combo_tx = combo_tx.clone();
            let current_press_id = Arc::new(AtomicU64::new(0));
            let current_press_id_for_match = Arc::clone(&current_press_id);
            let _match = match conn.add_match(rule, move |args: (u32, u32, bool), _conn, msg| {
                let (sym, states, is_press) = args;
                let member = msg.member();
                let member_str: String = member.as_ref().map(|m| m.to_string()).unwrap_or_default();
                log::debug!(
                    "[fcitx-hotkey] Signal {}: sym={}, states={}, isPress={}",
                    member_str, sym, states, is_press,
                );
                if let Some(member) = member {
                    if member == "DictationKeyEvent" {
                        let at = std::time::Instant::now();
                        let event = if is_press {
                            let press_id = crate::hotkey::next_press_id();
                            current_press_id_for_match.store(press_id, Ordering::SeqCst);
                            crate::hotkey::HotkeyEvent::Pressed { at, press_id }
                        } else {
                            current_press_id_for_match.store(0, Ordering::SeqCst);
                            crate::hotkey::HotkeyEvent::Released { at }
                        };
                        let _ = tx.send(event);
                    } else if member == "DictationKeyCombined" && is_press {
                        let press_id = current_press_id_for_match.load(Ordering::SeqCst);
                        if press_id != 0 {
                            let _ = combo_tx.send(press_id);
                        }
                    } else if member == "QaShortcutEvent" {
                        if is_press {
                            let _ = tx2.send(crate::hotkey::HotkeyEvent::QaShortcutPressed);
                        }
                    } else if member == "TranslationModifierEvent" {
                        if is_press {
                            let _ = tx2.send(crate::hotkey::HotkeyEvent::TranslationModifierPressed);
                        }
                    }
                }
                true
            }) {
                Ok(m) => m,
                Err(e) => {
                    log::warn!("[fcitx-hotkey] Failed to add match: {e}");
                    return;
                }
            };

            // 监听 fcitx5 的 NameOwnerChanged 信号，用于在 fcitx5 重启后重新同步。
            // dbus crate 的 MatchRule::parse 不支持 arg0 过滤，在回调里做匹配。
            let fcitx_rule = match dbus::message::MatchRule::parse(
                "type='signal',\
                 sender='org.freedesktop.DBus',\
                 interface='org.freedesktop.DBus',\
                 member='NameOwnerChanged'",
            ) {
                Ok(r) => r,
                Err(e) => {
                    log::warn!("[fcitx-hotkey] Invalid fcitx5 name watch rule: {e}");
                    return;
                }
            };

            // NOTE: NameOwnerChanged 捕获的是线程启动时的绑定快照。用户在
            // OpenLess 运行时改了快捷键且 fcitx5 恰好重启，重连会写入旧绑定。
            // 这是一个低概率场景（需要两个操作同时发生），暂时保留快照语义。
            // 要彻底解决需要把 Arc<PreferencesStore> 传给监听线程做实时读取。
            let binding_for_name = binding.clone();
            let custom_for_name = custom_trigger_key.clone();
            let qa_for_name = qa_trigger;
            let trans_for_name = translation_trigger;
            let _name_match = match conn.add_match(fcitx_rule, move |args: (String, String, String), _conn, _msg| {
                let (name, _old_owner, new_owner) = args;
                if name != "org.fcitx.Fcitx5" { return true; }
                if !new_owner.is_empty() {
                    // fcitx5 已启动（或重启），重新同步所有快捷键绑定。
                    // 把延迟+同步挪到独立线程：add_match 回调跑在 DBus 事件循环
                    // 线程里，sleep 会阻塞所有信号处理。
                    log::info!("[fcitx-hotkey] fcitx5 appeared on DBus, re-syncing bindings");
                    let b = binding_for_name.clone();
                    let c = custom_for_name.clone();
                    let q = qa_for_name;
                    let t = trans_for_name;
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_secs(1)); // 等插件完全加载
                        resync_main_binding(&b, c.as_deref());
                        sync_qa_binding(q);
                        sync_translation_binding(t);
                    });
                }
                true
            }) {
                Ok(m) => m,
                Err(e) => {
                    log::warn!("[fcitx-hotkey] Failed to add fcitx5 name watch: {e}");
                    return;
                }
            };

            // 初始同步：等待 fcitx5 可用（最多重试 10 次，每次 3 秒）。
            for attempt in 0..10 {
                if fcitx5_name_has_owner(&conn) {
                    log::info!("[fcitx-hotkey] fcitx5 available, syncing initial bindings (attempt {attempt})");
                    resync_main_binding(&binding, custom_trigger_key.as_deref());
                    sync_qa_binding(qa_trigger);
                    sync_translation_binding(translation_trigger);
                    break;
                }
                if attempt == 0 {
                    log::info!("[fcitx-hotkey] fcitx5 not yet available, will retry...");
                }
                std::thread::sleep(Duration::from_secs(3));
            }

            // ⚠️ `_match` / `_name_match` 是 dbus::MsgMatch guard — drop 即注销。
            // Rust 中 `let _name = ...` 绑定生命周期正常（仅有 `let _ = ...` 才立即 drop），
            // 它们与 `loop {}` 在同一个闭包作用域内，事件循环期间不会提前析构。
            // 自动化审核对此的 HIGH 报告是误判。
            log::info!("[fcitx-hotkey] Listening for OpenLess1 signals");
            loop {
                if let Err(e) = conn.process(Duration::from_millis(500)) {
                    log::warn!("[fcitx-hotkey] DBus process error: {e}");
                    break;
                }
            }
        })
        .ok();
}

/// 检查 fcitx5 插件是否已安装到系统路径。
///
/// 所有 Linux 格式（deb/rpm/AppImage）的插件安装都在打包时完成
///（`scripts/inject-fcitx5-plugin.sh`），此处仅确认文件存在。
/// 未安装时输出警告，不做任何文件 I/O。
#[cfg(target_os = "linux")]
pub fn ensure_plugin_installed(_app: &tauri::AppHandle) {
    // fcitx5 在不同发行版的 lib 路径不同，同时支持用户 XDG 安装
    let lib_dirs = [
        "/usr/lib/x86_64-linux-gnu/fcitx5", // Debian multiarch
        "/usr/lib64/fcitx5",                // RPM 64-bit
        "/usr/lib/fcitx5",                  // 通用回退
    ];
    let system_conf = std::path::Path::new("/usr/share/fcitx5/addon/openless.conf");

    // 用户 XDG 安装：~/.local/ 下自编译安装的版本
    let (user_so, user_conf) = if let Ok(home) = std::env::var("HOME") {
        let home = std::path::PathBuf::from(home);
        (
            home.join(".local/lib/fcitx5/libopenless.so"),
            home.join(".local/share/fcitx5/addon/openless.conf"),
        )
    } else {
        (std::path::PathBuf::new(), std::path::PathBuf::new())
    };

    let conf_ok = user_conf.exists() || system_conf.exists();
    let system_so_found = lib_dirs
        .iter()
        .find(|dir| std::path::Path::new(dir).join("libopenless.so").exists());
    let so_ok = user_so.exists() || system_so_found.is_some();

    // 用户手动安装过 ~/.local/ 版本，同时系统路径也有（deb 注入的）→
    // fcitx5 优先加载用户路径的旧版，系统新版被忽略。
    // 提醒用户删除 ~/.local/ 的旧插件。
    if user_so.exists() && system_so_found.is_some() {
        log::warn!(
            "[fcitx] fcitx5 plugin found in both ~/.local/ and system paths. \
             fcitx5 will load the ~/.local/ version first, which may be outdated. \
             Remove it if you want to use the system-installed version: rm -f {}",
            user_so.display()
        );
    }

    if !conf_ok {
        log::warn!(
            "[fcitx] fcitx5 addon config not found. \
             The OpenLess package may be incomplete."
        );
        return;
    }

    if !so_ok {
        log::warn!(
            "[fcitx] fcitx5 plugin .so not found in {:?} or {:?}. \
             The OpenLess package may be incomplete.",
            lib_dirs,
            user_so
        );
    }
}

/// 同步主听写热键：自定义组合键走 SetCustomDictationTrigger，预设修饰键走 SetHotkeyRaw。
fn resync_main_binding(binding: &crate::types::HotkeyBinding, custom_trigger_key: Option<&str>) {
    if let Some(key_string) = custom_trigger_key {
        if !key_string.is_empty() {
            match set_custom_dictation_trigger(key_string) {
                Ok(()) => log::info!("[fcitx] Resynced custom dictation trigger '{key_string}'"),
                Err(e) => log::warn!("[fcitx] Failed to resync custom dictation trigger: {e}"),
            }
            return;
        }
    }
    sync_binding_to_plugin(binding);
}

/// 检查 fcitx5 是否在 DBus 上注册了名称（即 fcitx5 进程是否在运行且 DBus 模块已加载）。
fn fcitx5_name_has_owner(conn: &dbus::blocking::SyncConnection) -> bool {
    use dbus::blocking::BlockingSender;
    let msg = match dbus::Message::new_method_call(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "NameHasOwner",
    ) {
        Ok(m) => m,
        Err(_) => return false,
    };
    let msg = msg.append1("org.fcitx.Fcitx5");
    match conn.send_with_reply_and_block(msg, Duration::from_secs(1)) {
        Ok(reply) => reply.read1::<bool>().unwrap_or(false),
        Err(_) => false,
    }
}
