//! Stamp the product version into the Linux egui binary.
//!
//! The egui host and the Tauri app ship as one product, so the version shown in
//! the UI must track `package.json` — the release workflow derives the package
//! version from the same file. `CARGO_PKG_VERSION` only carries the internal
//! crate version (`0.1.0`), which is not what users should see.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=../package.json");
    let version = std::env::var("OPENLESS_LINUX_VERSION")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(read_package_version)
        .unwrap_or_else(|| std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into()));
    println!("cargo:rustc-env=OPENLESS_APP_VERSION={version}");
}

/// Extract the top-level `version` field. The build script stays
/// dependency-free on purpose, so this is a small targeted scan rather than a
/// JSON parse: `package.json` declares exactly one `"version"` key.
fn read_package_version() -> Option<String> {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").ok()?);
    let raw = std::fs::read_to_string(manifest.join("../package.json")).ok()?;
    let key = "\"version\"";
    let rest = &raw[raw.find(key)? + key.len()..];
    let rest = &rest[rest.find(':')? + 1..];
    let rest = &rest[rest.find('"')? + 1..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}
