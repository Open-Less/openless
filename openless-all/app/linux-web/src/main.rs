//! Linux 宿主：GTK 窗口 + WebKitGTK 渲染**官方 React 界面**（不装 Tauri）。
//!
//! 桥的形状与 `src/lib/harmonyBridge.ts` 完全一致：
//!   JS → native : window.webkit.messageHandlers.openless.postMessage(JSON)
//!   native → JS : window.__openlessResolve(id, JSON)
//!
//! 静态资源用一个只在 127.0.0.1 上监听的小 HTTP 服务提供（WebKit 拒绝 file://
//! 下的 ES module / fetch），这也是 Tauri/Cog 之类的常规做法。
use gtk::prelude::*;
use javascriptcore::ValueExt;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use serde_json::{json, Value};
use webkit2gtk::{SettingsExt, UserContentManager, UserContentManagerExt, WebViewExt};

fn main() {
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
    println!("[linux-web] serving {} at {}", dist.display(), url);

    let app = gtk::Application::new(Some("com.openless.linuxweb"), Default::default());
    app.connect_activate(move |app| build_window(app, &url));
    app.run();
}

fn build_window(app: &gtk::Application, url: &str) {
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
        let (ok, payload) = match dispatch(&command, &args) {
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

    window.set_child(Some(&view));
    view.load_uri(url);
    window.show_all();
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

fn dispatch(command: &str, _args: &Value) -> Result<Value, String> {
    let dir = data_dir();
    match command {
        "get_startup_snapshot" => Ok(json!({
            "contractVersion": "2.0.0",
            "backend": { "running": true }
        })),
        "get_settings" | "get_settings_snapshot" => read_json(&dir.join("preferences.json"))
            .ok_or_else(|| "读不到 preferences.json".to_string()),
        "set_settings" | "save_settings" => Ok(json!({ "saved": true })),
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
