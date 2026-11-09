#!/usr/bin/env python3
"""Run the native recorder tests without building Tauri/MLX or fetching ASR submodules.

The bridge trait below matches the only core interface used by recorder.rs.
This checks the production recorder module; it does not replace a desktop cargo check.
Pass --check to compile the same tests for a cross target without linking or running them.
"""
import json
from pathlib import Path
import subprocess
import sys
import tempfile

recorder = Path(__file__).resolve().parents[1] / "src-tauri/src/recorder.rs"
cargo_args = sys.argv[1:]
check_only = "--check" in cargo_args
if check_only:
    cargo_args.remove("--check")
with tempfile.TemporaryDirectory(prefix="openless-recorder-tests-") as directory:
    root = Path(directory)
    (root / "src").mkdir()
    (root / "Cargo.toml").write_text("""[package]
name = "openless-recorder-tests"
version = "0.1.0"
edition = "2021"
[dependencies]
parking_lot = "0.12"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"
log = "0.4"
""" + "cpal = { version = \"=0.15.3\", path = " + json.dumps((recorder.parents[1] / "vendor/cpal").as_posix()) + " }\n"
        + "micyou-dsp = { path = " + json.dumps((recorder.parents[2] / "crates/micyou-dsp").as_posix()) + " }\n"
        + "[build-dependencies]\ncc = \"1\"\n[target.'cfg(target_os = \"macos\")'.dependencies]\ncoreaudio-sys = \"0.2\"\n")
    (root / "build.rs").write_text(
        'fn main() { if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {\n'
        + 'cc::Build::new().file(' + json.dumps(str(recorder.with_name("playback_reference_macos.m")))
        + ').flag("-fobjc-arc").flag("-mmacosx-version-min=14.0").compile("openless_playback_reference");\n'
        + 'let compiler=cc::Build::new().get_compiler(); let resource=std::process::Command::new(compiler.path()).arg("-print-resource-dir").output().expect("find Apple compiler runtime"); assert!(resource.status.success());\n'
        + 'let runtime=std::path::PathBuf::from(String::from_utf8(resource.stdout).unwrap().trim()).join("lib/darwin"); println!("cargo:rustc-link-search=native={}",runtime.display()); println!("cargo:rustc-link-lib=static=clang_rt.osx");\n'
        + 'println!("cargo:rustc-link-lib=framework=Foundation"); println!("cargo:rustc-link-lib=framework=CoreAudio"); } }\n'
    )
    (root / "src/lib.rs").write_text(
        "extern crate self as openless_core;\n"
        "#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum BackendErrorCode { InvalidArgument }\n"
        "#[derive(Debug, Clone)] pub struct BackendError { pub code: BackendErrorCode, pub message: String }\n"
        "impl BackendError { pub fn new(code: BackendErrorCode, message: impl Into<String>) -> Self { Self { code, message: message.into() } } }\n"
        + "#[path = " + json.dumps(str(recorder.parents[2] / "crates/openless-core/src/audio_dsp.rs"), ensure_ascii=False) + "]\npub mod audio_dsp;\n"
        "pub use audio_dsp::{AecMode, AudioDspConfig, AudioDspStage, NoiseReduction};\n"
        + "#[path = " + json.dumps(str(recorder.parents[2] / "crates/openless-core/src/audio.rs"), ensure_ascii=False) + "]\npub mod audio;\n"
        "pub use audio::{pcm_f32_to_i16_le, AudioProcessingConfig, AudioProcessor, FarEndQueue, ReferenceAudioBuffer};\n"
        "pub trait AudioConsumer: Send + Sync { fn consume_pcm_chunk(&self, pcm: &[u8]); }\n"
        + "#[path = " + json.dumps(str(recorder.with_name("issue1266_debug.rs")), ensure_ascii=False) + "]\nmod issue1266_debug;\n"
        + "#[path = " + json.dumps(str(recorder), ensure_ascii=False) + "]\npub mod recorder;\n"
        + "#[path = " + json.dumps(str(recorder.with_name("native_audio_cue.rs")), ensure_ascii=False) + "]\npub mod native_audio_cue;\n"
    )
    subprocess.run(
        ["cargo", "check" if check_only else "test", "--manifest-path", str(root / "Cargo.toml"),
         *(["--tests"] if check_only else []), *cargo_args],
        check=True,
    )
