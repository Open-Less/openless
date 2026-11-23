//! macOS 上编译系统音频 tap 的 Objective-C 部分。
fn main() {
    println!("cargo:rerun-if-changed=src/audio/capture/macos_tap.m");
    println!("cargo:rerun-if-changed=src/macos/guide.m");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new()
            .file("src/audio/capture/macos_tap.m")
            .flag("-fobjc-arc")
            .flag("-mmacosx-version-min=14.0")
            .compile("openless_platform_macos_tap");

        println!("cargo:rerun-if-changed=src/macos/guide.m");
        cc::Build::new()
            .file("src/macos/guide.m")
            .flag("-fobjc-arc")
            .flag("-mmacosx-version-min=14.0")
            .compile("openless_platform_accessibility_guide");
    }
}
