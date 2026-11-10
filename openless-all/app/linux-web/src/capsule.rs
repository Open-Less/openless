//! 录音胶囊：一个**不给焦点**的 layer surface（`zwlr_layer_shell_v1`）。
//!
//! 为什么不用普通窗口：capsule 出现的时刻用户正在别的应用里说话，一旦抢焦点，
//! fcitx5 的插入目标就变成我们自己。`gtk-layer-shell` 允许
//! `keyboard_mode = None`，位置也用 anchor + margin 精确固定（贴底居中）。
//!
//! 页面：同一个 dist，用 `?capsule=1` 让官方 React 走 `Capsule.tsx`；
//! 状态由事件桥推（`dictation:state` / `dictation:level`）。
use gtk::prelude::*;
use gtk_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use webkit2gtk::{SettingsExt, WebViewExt};
use std::sync::Arc;

pub struct CapsuleWindow {
    pub window: gtk::ApplicationWindow,
    pub view: webkit2gtk::WebView,
}

/// 建一个胶囊窗口（不 present，调用方按状态显示/隐藏）。
pub fn build(
    app: &gtk::Application,
    url: &str,
    width: i32,
    height: i32,
    bottom_margin: i32,
) -> CapsuleWindow {
    let window = gtk::ApplicationWindow::new(app);
    window.set_title("OpenLess Capsule");
    window.set_default_size(width, height);
    window.set_decorated(false);
    // 透明底：页面自己画药丸，其余透出桌面。
    window.set_app_paintable(true);
    if let Some(screen) = gtk::prelude::GtkWindowExt::screen(&window) {
        if let Some(visual) = screen.rgba_visual() {
            window.set_visual(Some(&visual));
        }
    }
    let css = gtk::CssProvider::new();
    css.load_from_data(b"window { background-color: transparent; }");
    if let Some(screen) = gtk::prelude::GtkWindowExt::screen(&window) {
        gtk::StyleContext::add_provider_for_screen(
            &screen,
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    window.init_layer_shell();
    window.set_layer(Layer::Overlay);
    window.set_anchor(Edge::Bottom, true);
    window.set_anchor(Edge::Left, true);
    window.set_anchor(Edge::Right, true);
    window.set_layer_shell_margin(Edge::Bottom, bottom_margin);
    window.set_keyboard_mode(KeyboardMode::None);
    window.set_namespace("openless-capsule");

    let view = webkit2gtk::WebView::new();
    if let Some(settings) = webkit2gtk::WebViewExt::settings(&view) {
        settings.set_enable_webgl(true);
    }
    view.set_background_color(&gtk::gdk::RGBA::new(0.0, 0.0, 0.0, 0.0));
    view.load_uri(url);
    window.add(&view);
    CapsuleWindow { window, view }
}

/// 把胶囊挂到事件桥上：`dictation:state` 决定显隐，电平取自同一个状态快照
/// （`DictationStateSnapshot.level`，Core 每次电平变化都会 publish 一次）。
///
/// 结构说明：GTK 对象是 `!Send`，不能跨 await 持有，所以
/// **工作线程**只把 JSON 字符串丢进 glib 通道，**主线程**再拿着窗口去求值。
pub fn attach_events(
    capsule: Arc<CapsuleWindow>,
    runtime: Arc<tokio::runtime::Runtime>,
    backend: Arc<openless_core::OpenLessBackend>,
) {
    #[allow(deprecated)]
    let (sender, receiver) = gtk::glib::MainContext::channel::<(bool, String)>(gtk::glib::Priority::default());

    receiver.attach(None, move |(visible, js): (bool, String)| {
        if visible {
            capsule.window.show_all();
        } else {
            capsule.window.hide();
        }
        capsule.view.evaluate_javascript(
            &js,
            None::<&str>,
            None::<&str>,
            None::<&webkit2gtk::gio::Cancellable>,
            |_| {},
        );
        gtk::glib::ControlFlow::Continue
    });

    std::thread::spawn(move || {
        let mut subscription = backend.subscribe();
        loop {
            let event = match runtime.block_on(subscription.recv()) {
                Ok(event) => event,
                Err(_) => continue,
            };
            let openless_core::BackendEventKind::DictationStateChanged(state) = &event.kind else {
                continue;
            };
            let visible = !matches!(state.phase, openless_core::DictationPhase::Idle);
            let js = format!(
                "window.__openlessEvent && window.__openlessEvent(\"dictation:level\", {});",
                serde_json::json!({ "level": state.level })
            );
            if sender.send((visible, js)).is_err() {
                break;
            }
        }
    });
}
