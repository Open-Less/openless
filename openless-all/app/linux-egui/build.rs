//! Stamp the product version into the Linux egui binary.
//!
//! The egui host and the Tauri app ship as one product, so the version shown in
//! the UI must track `package.json` — the release workflow derives the package
//! version from the same file. `CARGO_PKG_VERSION` only carries the internal
//! crate version (`0.1.0`), which is not what users should see.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=../package.json");
    println!("cargo:rerun-if-changed=package-revision");
    println!("cargo:rerun-if-env-changed=OPENLESS_LINUX_VERSION");
    let base = read_package_version().expect("package.json must declare the product version");
    let revision = std::fs::read_to_string(
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("package-revision"),
    )
    .expect("Linux package revision is required");
    let revision = revision.trim();
    assert!(
        !revision.is_empty() && revision.bytes().all(|c| c.is_ascii_digit()),
        "invalid Linux package revision"
    );
    let expected = format!("{}-{revision}", base.split('+').next().unwrap_or(&base));
    if let Ok(version) = std::env::var("OPENLESS_LINUX_VERSION") {
        assert_eq!(version, expected, "binary and package versions must match");
    }
    println!("cargo:rustc-env=OPENLESS_APP_VERSION={expected}");
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
