//! macOS 上编译系统音频 tap 的 Objective-C 部分。
fn main() {
    println!("cargo:rerun-if-changed=src/audio/capture/macos_tap.m");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new()
            .file("src/audio/capture/macos_tap.m")
            .flag("-fobjc-arc")
            .compile("openless_platform_macos_tap");
    }
}
