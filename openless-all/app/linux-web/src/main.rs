//! Linux 宿主：GTK 窗口 + WebKitGTK 渲染**官方 React 界面**（不装 Tauri）。
//!
//! 桥的形状与 `src/lib/harmonyBridge.ts` 完全一致：
//!   JS → native : window.webkit.messageHandlers.openless.postMessage(JSON)
//!   native → JS : window.__openlessResolve(id, JSON)
//!
//! 静态资源用一个只在 127.0.0.1 上监听的小 HTTP 服务提供（WebKit 拒绝 file://
//! 下的 ES module / fetch），这也是 Tauri/Cog 之类的常规做法。
mod events;
mod host;
mod hotkeys;

use gtk::prelude::*;
use javascriptcore::ValueExt;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use serde_json::{json, Value};
use webkit2gtk::{SettingsExt, UserContentManager, UserContentManagerExt, WebViewExt};

struct Host {
    backend: Option<std::sync::Arc<openless_core::OpenLessBackend>>,
    runtime: Option<std::sync::Arc<tokio::runtime::Runtime>>,
    settings_runtime: Option<std::sync::Arc<dyn openless_core::SettingsRuntime>>,
}

fn main() {
    let host = init_host();
    let dist = std::env::var("OPENLESS_DIST")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("dist"));
    let port = match serve_dir(dist.clone()) {
        Ok(port) => port,
        Err(error) => {
            eprintln!("[linux-web] 静态资源目录不可用（{}）：{error}", dist.display());
            std::process::exit(1);
        }
    };
    let url = format!("http://127.0.0.1:{port}/index.html");
    // 宿主传进来的桥 socket：窗口层后续通过它订阅事件（目前只记录，便于排障）。
    if let Some(index) = std::env::args().position(|arg| arg == "--host-socket") {
        if let Some(socket) = std::env::args().nth(index + 1) {
            println!("[linux-web] 宿主桥 socket: {socket}");
        }
    }
    println!("[linux-web] serving {} at {}", dist.display(), url);

    let app = gtk::Application::new(Some("com.openless.linuxweb"), Default::default());
    let host = std::sync::Arc::new(host);
    app.connect_activate(move |app| build_window(app, &url, host.clone()));
    app.run();
}

fn build_window(app: &gtk::Application, url: &str, host: std::sync::Arc<Host>) {
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("OpenLess")
        .default_width(1180)
        .default_height(820)
        .build();

    let manager = UserContentManager::new();
    manager.register_script_message_handler("openless");

    let view = webkit2gtk::WebView::builder()
        .user_content_manager(&manager)
        .build();

    if let Some(settings) = WebViewExt::settings(&view) {
        settings.set_enable_webgl(true);
        settings.set_enable_developer_extras(true);
    }
    // 与产品一致的浅色底；页面自己画卡片。
    view.set_background_color(&gtk::gdk::RGBA::new(0.96, 0.96, 0.95, 1.0));

    let host_for_ipc = host.clone();
    let view_for_ipc = view.clone();
    manager.connect_script_message_received(Some("openless"), move |_m, result| {
        let Some(value) = result.js_value() else { return };
        let raw = value.to_str().to_string();
        let Ok(message) = serde_json::from_str::<Value>(&raw) else {
            eprintln!("[linux-web] 无法解析桥消息: {raw}");
            return;
        };
        let id = message["id"].as_str().unwrap_or_default().to_string();
        let command = message["command"].as_str().unwrap_or_default().to_string();
        let args: Value = message["args"]
            .as_str()
            .and_then(|raw| serde_json::from_str(raw).ok())
            .unwrap_or_else(|| json!({}));
        let (ok, payload) = match dispatch(&host_for_ipc, &command, &args) {
            Ok(value) => (true, json!({ "ok": true, "value": value })),
            Err(error) => (false, json!({ "ok": false, "error": error })),
        };
        let js = format!(
            "window.__openlessResolve({}, {});",
            json!(id),
            json!(payload.to_string())
        );
        let _ = ok;
        view_for_ipc.evaluate_javascript(
            &js,
            None::<&str>,
            None::<&str>,
            None::<&webkit2gtk::gio::Cancellable>,
            |_| {},
        );
    });

    // 排障用：OPENLESS_WEB_TOPMOST=1 让窗口浮在最前（截图/对比时用）。
    if std::env::var("OPENLESS_WEB_TOPMOST").ok().as_deref() == Some("1") {
        window.set_keep_above(true);
    }
    window.set_child(Some(&view));
    view.load_uri(url);
    window.show_all();
    // 托盘与窗口显隐：窗口关掉只是隐藏，进程继续常驻（与 Tauri 版行为一致）。
    window.connect_delete_event(|window, _| {
        window.hide();
        gtk::glib::Propagation::Stop
    });
    host::start_tray(window.clone());
    // 后端就绪后立刻开始把事件推给页面（听写状态 / 历史变更 / QA 电平等）。
    {
        let backend = host.backend.clone();
        let runtime = host.runtime.clone();
        if let (Some(backend), Some(runtime)) = (backend.clone(), runtime.clone()) {
            events::spawn_backend_events(backend.clone(), runtime.clone(), view.clone());
            hotkeys::spawn(backend, runtime, window.clone());
        }
    }
}

// ── 命令分发 ────────────────────────────────────────────────────────────────

fn data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("OPENLESS_DATA_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share"));
    home.join("OpenLess")
}

fn read_json(path: &Path) -> Option<Value> {
    std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok())
}

fn init_host() -> Host {
    let mut host = Host { backend: None, runtime: None, settings_runtime: None };
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => std::sync::Arc::new(runtime),
        Err(error) => {
            eprintln!("[linux-web] tokio 运行时创建失败：{error}");
            return host;
        }
    };
    let home_dir = std::env::var("HOME").ok().map(PathBuf::from);
    let config = openless_core::BackendConfig {
        data_dir: data_dir(),
        home_dir,
        ..Default::default()
    };
    let _guard = runtime.enter();
    match openless_linux_egui::LinuxBackendBuilder::from_shared_providers(config)
        .and_then(|builder| builder.build())
    {
        Ok(runtime_parts) => {
            println!("[linux-web] 后端就绪（openless-core 已装配）");
            host.backend = Some(runtime_parts.backend);
            host.settings_runtime = Some(runtime_parts.settings_runtime);
        }
        Err(error) => eprintln!("[linux-web] 后端装配失败：{error}"),
    }
    host.runtime = Some(runtime);
    host
}

fn channel_kind(kind: Option<&str>) -> Option<openless_core::ChannelKind> {
    match kind {
        Some("asr") => Some(openless_core::ChannelKind::Asr),
        Some("llm") => Some(openless_core::ChannelKind::Llm),
        _ => None,
    }
}

fn dispatch(host: &Host, command: &str, _args: &Value) -> Result<Value, String> {
    let dir = data_dir();
    let backend = host.backend.as_deref();
    match command {
        "get_startup_snapshot" => Ok(json!({
            "contractVersion": "2.0.0",
            "backend": { "running": backend.is_some() }
        })),
        "get_settings" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            Ok(serde_json::to_value(backend.get_preferences()).unwrap_or(Value::Null))
        }
        "get_settings_snapshot" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            Ok(json!({
                "preferences": serde_json::to_value(backend.get_preferences()).unwrap_or(Value::Null),
                "revision": backend.snapshot().preferences_revision,
            }))
        }
        "get_credentials" => {
            let (backend, runtime) = match (backend, host.runtime.as_ref()) {
                (Some(backend), Some(runtime)) => (backend, runtime),
                _ => return Err("后端未就绪".to_string()),
            };
            let status = runtime
                .block_on(backend.get_credentials_status())
                .map_err(|error| error.to_string())?;
            Ok(serde_json::to_value(status).unwrap_or(Value::Null))
        }
        // 设置写回：走 core 的 update_settings（带 revision 冲突检测），
        // 运行时效果（开机自启 / 热键 / 渠道切换）由 LinuxSettingsRuntime 负责。
        "set_settings" | "save_settings" | "update_setting_fields" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host
                .settings_runtime
                .as_deref()
                .ok_or_else(|| "设置运行时未就绪".to_string())?;
            let edits: std::collections::BTreeMap<String, Value> = _args
                .get("edits")
                .and_then(|value| serde_json::from_value(value.clone()).ok())
                .unwrap_or_default();
            let revision = backend.snapshot().preferences_revision;
            let draft = openless_core::preference_patch::patch_preferences(
                &backend.get_preferences(),
                &edits,
            )
            .map_err(|error| error.to_string())?;
            backend
                .update_settings(
                    draft,
                    openless_core::SettingsUpdateOptions {
                        expected_preferences_revision: Some(revision),
                        ..openless_core::SettingsUpdateOptions::STRICT
                    },
                    runtime,
                )
                .map_err(|error| error.to_string())?;
            Ok(json!({
                "preferences": serde_json::to_value(backend.get_preferences()).unwrap_or(Value::Null),
                "revision": backend.snapshot().preferences_revision,
            }))
        }
        "list_history" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let entries = backend.list_history().map_err(|error| error.to_string())?;
            serde_json::to_value(entries).map_err(|error| error.to_string())
        }
        "get_activity_stats" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let days = backend
                .list_activity()
                .map_err(|error| error.to_string())?;
            serde_json::to_value(days).map_err(|error| error.to_string())
        }
        "list_provider_descriptors" => {
            let kind = match _args.get("kind").and_then(|value| value.as_str()) {
                Some("asr") => openless_core::ProviderKind::Asr,
                Some("llm") => openless_core::ProviderKind::Llm,
                Some("omni") => openless_core::ProviderKind::Omni,
                _ => return Err("kind 必须是 asr / llm".to_string()),
            };
            serde_json::to_value(openless_core::provider_rules::provider_descriptors(kind))
                .map_err(|error| error.to_string())
        }
        // ── 渠道 ───────────────────────────────────────────────────────
        "list_channels" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone();
            let runtime = runtime.ok_or_else(|| "运行时未就绪".to_string())?;
            let kind = channel_kind(_args.get("kind").and_then(|v| v.as_str()))
                .ok_or_else(|| "kind 必须是 asr / llm".to_string())?;
            let channels = runtime
                .block_on(backend.list_channels(kind))
                .map_err(|error| error.to_string())?;
            serde_json::to_value(channels).map_err(|error| error.to_string())
        }
        "create_channel" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone();
            let runtime = runtime.ok_or_else(|| "运行时未就绪".to_string())?;
            let kind = channel_kind(_args.get("kind").and_then(|v| v.as_str()))
                .ok_or_else(|| "kind 必须是 asr / llm".to_string())?;
            let name = _args
                .get("name")
                .and_then(|value| value.as_str())
                .unwrap_or("新渠道")
                .to_string();
            let provider_type = _args
                .get("providerType")
                .or_else(|| _args.get("provider_type"))
                .and_then(|value| value.as_str())
                .unwrap_or("custom")
                .to_string();
            let channel = runtime
                .block_on(backend.create_channel(kind, name, provider_type))
                .map_err(|error| error.to_string())?;
            serde_json::to_value(channel).map_err(|error| error.to_string())
        }
        "delete_channel" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone();
            let runtime = runtime.ok_or_else(|| "运行时未就绪".to_string())?;
            let kind = channel_kind(_args.get("kind").and_then(|v| v.as_str()))
                .ok_or_else(|| "kind 必须是 asr / llm".to_string())?;
            let id = _args
                .get("id")
                .and_then(|value| value.as_str())
                .ok_or_else(|| "缺少 id".to_string())?
                .to_string();
            runtime
                .block_on(backend.delete_channel(kind, id))
                .map_err(|error| error.to_string())?;
            Ok(json!({ "deleted": true }))
        }
        "set_channel_enabled" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone();
            let runtime = runtime.ok_or_else(|| "运行时未就绪".to_string())?;
            let kind = channel_kind(_args.get("kind").and_then(|v| v.as_str()))
                .ok_or_else(|| "kind 必须是 asr / llm".to_string())?;
            let id = _args
                .get("id")
                .and_then(|value| value.as_str())
                .ok_or_else(|| "缺少 id".to_string())?
                .to_string();
            let enabled = _args.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
            runtime
                .block_on(backend.set_channel_enabled(kind, id, enabled))
                .map_err(|error| error.to_string())?;
            Ok(json!({ "enabled": enabled }))
        }
        // ── 凭据读写（映射表在 core: credential_accounts::credential_key）──
        "read_credential" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let account = _args
                .get("account")
                .and_then(|value| value.as_str())
                .ok_or_else(|| "缺少 account".to_string())?;
            let provider = _args
                .get("provider")
                .and_then(|value| value.as_str())
                .map(str::to_string);
            let key = openless_core::credential_accounts::credential_key(account, provider)
                .map_err(|error| error.to_string())?;
            let value = runtime
                .block_on(backend.read_credential(key))
                .map_err(|error| error.to_string())?;
            Ok(json!(value.map(|secret| secret.into_exposed())))
        }
        "set_credential" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let account = _args
                .get("account")
                .and_then(|value| value.as_str())
                .ok_or_else(|| "缺少 account".to_string())?;
            let provider = _args
                .get("provider")
                .and_then(|value| value.as_str())
                .map(str::to_string);
            let value = _args
                .get("value")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string();
            let key = openless_core::credential_accounts::credential_key(account, provider)
                .map_err(|error| error.to_string())?;
            runtime
                .block_on(backend.set_credential(
                    key,
                    openless_core::SecretValue::new(value),
                ))
                .map_err(|error| error.to_string())?;
            Ok(json!({ "saved": true }))
        }
        "validate_provider_credentials" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let kind = _args
                .get("kind")
                .and_then(|value| value.as_str())
                .and_then(|kind| match kind {
                    "asr" => Some(openless_core::ProviderKind::Asr),
                    "llm" => Some(openless_core::ProviderKind::Llm),
                    "omni" => Some(openless_core::ProviderKind::Omni),
                    _ => None,
                })
                .ok_or_else(|| "kind 必须是 asr / llm / omni".to_string())?;
            let channel_id = _args
                .get("channelId")
                .or_else(|| _args.get("channel_id"))
                .and_then(|value| value.as_str())
                .map(str::to_string);
            let thinking_enabled = backend.get_preferences().llm_thinking_enabled;
            let result = runtime
                .block_on(backend.services().provider.validate(openless_core::ProviderRequest {
                    kind,
                    channel_id,
                    thinking_enabled,
                }))
                .map_err(|error| {
                    error
                        .details
                        .as_ref()
                        .and_then(|details| details.get("siwc"))
                        .map(|details| format!("siwc:{details}"))
                        .unwrap_or(error.message)
                })?;
            serde_json::to_value(result).map_err(|error| error.to_string())
        }
        "get_history" | "history_list" => {
            let history = read_json(&dir.join("history.json")).unwrap_or_else(|| json!([]));
            Ok(json!({ "entries": history, "revision": 0 }))
        }
        other => Err(format!("未实现的命令：{other}")),
    }
}

// ── 静态资源 ────────────────────────────────────────────────────────────────

fn serve_dir(root: PathBuf) -> std::io::Result<u16> {
    if !root.join("index.html").exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("{} 里没有 index.html", root.display()),
        ));
    }
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let root = root.clone();
            std::thread::spawn(move || {
                let _ = handle_request(stream, &root);
            });
        }
    });
    Ok(port)
}

fn handle_request(mut stream: TcpStream, root: &Path) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 || header.trim().is_empty() {
            break;
        }
    }
    let path = request_line.split_whitespace().nth(1).unwrap_or("/");
    let path = path.split('?').next().unwrap_or("/");
    let mut file_path = root.join(path.trim_start_matches('/'));
    if path.ends_with('/') || file_path.is_dir() {
        file_path.push("index.html");
    }
    let body = std::fs::read(&file_path).unwrap_or_default();
    let (status, mime) = if body.is_empty() {
        ("404 Not Found", "text/plain")
    } else {
        ("200 OK", mime_for(&file_path))
    };
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(&body)?;
    Ok(())
}

fn mime_for(path: &Path) -> &'static str {
    match path.extension().and_then(|ext| ext.to_str()).unwrap_or_default() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}
