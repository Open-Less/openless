//! Installation namespaces are fixed in the binary, independent of launch environment.
pub(crate) const DEV: bool = cfg!(all(feature = "dev-build", target_os = "macos"));
pub(crate) const DIRECTORY: &str = if DEV { "OpenLess Dev" } else { "OpenLess" };
pub(crate) const BUNDLE_ID: &str = if DEV {
    "com.openless.app.dev"
} else {
    "com.openless.app"
};
pub(crate) const SIWC_SERVICE: &str = if DEV {
    "com.openless.app.dev.siwc"
} else {
    "com.openless.app.siwc"
};
pub(crate) const UPDATES_ENABLED: bool = !DEV;

// Ask is available in every channel, including versions carrying legacy noask metadata.
// Native entry points retain their platform and user-preference checks.
pub(crate) fn ask_anything_available() -> bool {
    ask_anything_available_for_version(
        env!("CARGO_PKG_VERSION_PRE"),
        env!("CARGO_PKG_VERSION")
            .split_once('+')
            .map_or("", |(_, build)| build),
    )
}

pub(crate) fn ask_anything_available_for_version(_prerelease: &str, _build: &str) -> bool {
    true
}

pub(crate) fn platform_capabilities() -> openless_core::PlatformCapabilities {
    let mut capabilities = openless_core::PlatformCapabilities::current();
    capabilities.supports_auto_update &= UPDATES_ENABLED;
    capabilities
}

#[cfg(test)]
mod tests {
    #[test]
    fn ask_anything_is_reopened_for_release_dev_and_legacy_metadata() {
        for (prerelease, build) in [
            ("dev.noask.1", ""), ("Beta.2", "build.noask.20261123"),
            ("", ""), ("Dev.1", ""), ("dev.1", ""), ("Beta.2", "build.20261123"),
        ] {
            assert!(super::ask_anything_available_for_version(prerelease, build));
        }
    }
}
