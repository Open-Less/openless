// Prevents additional console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Work around WebKitGTK compositing bugs on Linux Wayland:
    // - WEBKIT_DISABLE_COMPOSITING_MODE=1 fixes "whole window unresponsive
    //   to clicks until maximize/restore" (tauri#9394)
    // - WEBKIT_DISABLE_DMABUF_RENDERER=1 fixes white/black screen on some
    //   GPU/driver combos (e.g. Nvidia + Debian)
    #[cfg(target_os = "linux")]
    {
        if std::env::var("WEBKIT_DISABLE_DMABUF_RENDERER").is_err() {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
        if std::env::var("WEBKIT_DISABLE_COMPOSITING_MODE").is_err() {
            std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
        }
    }

    openless_lib::run();
}
