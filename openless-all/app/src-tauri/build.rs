#[path = "src/build_target.rs"]
mod build_target;

fn main() {
    // In build.rs, `#[cfg(target_os)]` tests the build-script host, not
    // Cargo's target platform. Prefer Cargo's target OS; fall back to parsing
    // TARGET for old toolchains missing the variable, so cross-compiling
    // armv7 Android from a Linux host does not wrongly build the qwen-asr C
    // backend into the APK.
    let target = std::env::var("TARGET").unwrap_or_default();
    let target_os = build_target::classify_target_os(
        &target,
        std::env::var("CARGO_CFG_TARGET_OS").ok().as_deref(),
    );
    println!("cargo:warning=OpenLess build target={target}, target_os={target_os}");
    if target_os == "windows" {
        link_windows_common_controls_v6_manifest_dependency();
    }
    if matches!(target_os, "macos" | "linux") {
        build_qwen_asr(target_os);
    }

    if target_os == "macos" {
        link_macos_compiler_runtime();
        let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
        if target_arch == "aarch64" {
            compile_mlx_metallib_path_shim();
        }
    }

    if target_os == "android" {
        link_android_cpp_runtime();
    }

    tauri_build::build();
}

/// MLX uses `__builtin_available` for newer Metal APIs. Rust links with
/// `-nodefaultlibs`, so the availability helper from Apple compiler-rt must be
/// added explicitly or Apple Silicon release links fail on macOS 14 targets.
fn link_macos_compiler_runtime() {
    let compiler = cc::Build::new().get_compiler();
    let output = std::process::Command::new(compiler.path())
        .arg("-print-resource-dir")
        .output()
        .expect("failed to query the macOS compiler resource directory");
    if !output.status.success() {
        panic!(
            "macOS compiler did not return its resource directory (status {})",
            output.status
        );
    }

    let resource_dir = String::from_utf8(output.stdout)
        .expect("macOS compiler resource directory was not UTF-8")
        .trim()
        .to_owned();
    let runtime_dir = std::path::PathBuf::from(resource_dir)
        .join("lib")
        .join("darwin");
    if !runtime_dir.join("libclang_rt.osx.a").exists() {
        panic!(
            "macOS compiler runtime not found at {}",
            runtime_dir.display()
        );
    }

    println!("cargo:rustc-link-search=native={}", runtime_dir.display());
    println!("cargo:rustc-link-lib=static=clang_rt.osx");
}

/// Apple Silicon release bundles put mlx.metallib in Contents/Resources.
/// mlx-c by default only looks next to the executable; this adds a C entry
/// point that calls set_metallib_path.
fn compile_mlx_metallib_path_shim() {
    const SOURCE: &str = "src/asr/local/mlx_set_metallib_path.cpp";
    println!("cargo:rerun-if-changed={SOURCE}");
    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .file(SOURCE)
        .compile("openless_mlx_set_metallib_path");
}

/// cpal → oboe → oboe-sys compile C++; the final cdylib must link the NDK
/// libc++ explicitly.
fn link_android_cpp_runtime() {
    // oboe-ext is partially statically linked into libc++; link c++abi as well
    // to provide __cxa_pure_virtual and other ABI symbols.
    println!("cargo:rustc-link-lib=c++_static");
    println!("cargo:rustc-link-lib=c++abi");
}

fn link_windows_common_controls_v6_manifest_dependency() {
    let mut source_path = std::path::PathBuf::from(
        std::env::var_os("OUT_DIR").expect("OUT_DIR must be set by Cargo"),
    );
    source_path.push("common-controls-v6-manifest-dependency.c");
    std::fs::write(
        &source_path,
        r#"#pragma comment(linker, "/manifestdependency:\"type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'\"")
int openless_common_controls_v6_manifest_dependency_anchor = 0;
"#,
    )
    .expect("write common controls manifest dependency source");
    cc::Build::new()
        .file(&source_path)
        .compile("openless_common_controls_v6_manifest_dependency");
    println!(
        "cargo:rustc-link-arg=/INCLUDE:openless_common_controls_v6_manifest_dependency_anchor"
    );
}

/// Compiles the vendored Open-Less/qwen-asr C sources (macOS/Linux).
///
/// Equivalent to the upstream Makefile's `make blas`: BLAS acceleration via the
/// Accelerate framework; `USE_BLAS` + `ACCELERATE_NEW_LAPACK` are required
/// macros.
/// `-march=native` is deliberately NOT used — distributed binaries must stay
/// portable. The cc crate defaults to `-O2` in release; `-O3` bumps it one
/// level. NEON/AVX dispatch happens through `#ifdef`s in the sources.
fn build_qwen_asr(target_os: &str) {
    const VENDOR: &str = "vendor/qwen-asr";
    const SOURCES: &[&str] = &[
        "qwen_asr.c",
        "qwen_asr_kernels.c",
        "qwen_asr_kernels_generic.c",
        "qwen_asr_kernels_neon.c",
        "qwen_asr_kernels_avx.c",
        "qwen_asr_audio.c",
        "qwen_asr_encoder.c",
        "qwen_asr_decoder.c",
        "qwen_asr_tokenizer.c",
        "qwen_asr_safetensors.c",
    ];

    let mut build = cc::Build::new();
    build
        .include(VENDOR)
        .flag("-O3")
        .flag("-ffast-math")
        // Upstream builds with `-Wall -Wextra`; qwen-asr is treated as a
        // third-party dependency, so silence its irrelevant warnings to keep
        // build log noise from drowning out our own warnings.
        .flag("-Wno-unused-parameter")
        .flag("-Wno-unused-variable")
        .flag("-Wno-unused-function")
        .flag("-Wno-sign-compare")
        .warnings(false);

    if target_os == "macos" {
        build
            .define("USE_BLAS", None)
            .define("ACCELERATE_NEW_LAPACK", None);
    }

    for src in SOURCES {
        let path = format!("{}/{}", VENDOR, src);
        println!("cargo:rerun-if-changed={}", path);
        build.file(path);
    }
    println!("cargo:rerun-if-changed={}/qwen_asr.h", VENDOR);

    build.compile("qwen_asr");

    // BLAS = Accelerate
    if target_os == "macos" {
        println!("cargo:rustc-link-lib=framework=Accelerate");
    }

    // Linux does not depend on the distribution's OpenBLAS dev package; use the
    // C engine's generic CPU kernels first.
    if target_os == "linux" {
        println!("cargo:rustc-link-lib=m");
        println!("cargo:rustc-link-lib=pthread");
    }

    // Apple Speech local ASR (issue #574): apple_speech_provider uses
    // SFSpeechRecognizer / SFSpeechURLRecognitionRequest; symbols live in
    // Speech.framework.
    if target_os == "macos" {
        println!("cargo:rustc-link-lib=framework=Speech");
    }
}
