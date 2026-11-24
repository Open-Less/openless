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

        // 端侧 Gemma 适配器（Swift + Core ML）。只在 arm64 macOS 且启用实验引擎时编译。
        let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
        if arch == "aarch64" && std::env::var_os("CARGO_FEATURE_ASR_EXPERIMENTAL").is_some() {
            build_experimental_gemma();
            link_macos_compiler_runtime();
        }
    }
}

/// 编译 Swift 的 Core ML Gemma 适配器，并链接它需要的框架。
fn build_experimental_gemma() {
    const SOURCE: &str = "src/asr/experimental/gemma.swift";
    println!("cargo:rerun-if-changed={SOURCE}");
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let status = std::process::Command::new("xcrun")
        .args([
            "swiftc",
            "-O",
            "-parse-as-library",
            "-emit-library",
            "-static",
            "-module-name",
            "OpenlessGemma",
            "-target",
            "arm64-apple-macosx14.0",
            SOURCE,
            "-o",
        ])
        .arg(out.join("libOpenlessGemma.a"))
        .status()
        .expect("Swift compiler is required for the experimental Core ML adapter");
    assert!(
        status.success(),
        "experimental Core ML adapter failed to compile"
    );
    let swift = std::process::Command::new("xcrun")
        .args(["--find", "swiftc"])
        .output()
        .expect("find swiftc");
    let compiler = std::path::PathBuf::from(String::from_utf8(swift.stdout).unwrap().trim());
    let libraries = compiler
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("lib/swift/macosx");
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-search=native={}", libraries.display());
    println!("cargo:rustc-link-lib=static=OpenlessGemma");
    println!("cargo:rustc-link-lib=framework=CoreML");
    println!("cargo:rustc-link-lib=framework=Accelerate");
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
}

/// Apple 适配器用了 availability 检查；Rust 用 `-nodefaultlibs` 链接，
/// 需要显式补 compiler-rt 里的 availability helper，否则 Apple Silicon 发布链接失败。
fn link_macos_compiler_runtime() {
    let compiler = cc::Build::new().get_compiler();
    let output = std::process::Command::new(compiler.path())
        .arg("-print-resource-dir")
        .output()
        .expect("failed to query the macOS compiler resource directory");
    if !output.status.success() {
        panic!("macOS compiler did not return its resource directory");
    }
    let resource_dir = String::from_utf8(output.stdout).unwrap_or_default();
    let resource_dir = resource_dir.trim();
    if resource_dir.is_empty() {
        return;
    }
    println!(
        "cargo:rustc-link-search=native={resource_dir}/lib/darwin"
    );
}
