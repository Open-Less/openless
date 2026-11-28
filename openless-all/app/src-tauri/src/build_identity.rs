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

// Both release and Dev installations honor the Cargo version's noask marker.
pub(crate) fn ask_anything_available() -> bool {
    ask_anything_available_for_version(
        env!("CARGO_PKG_VERSION_PRE"),
        env!("CARGO_PKG_VERSION")
            .split_once('+')
            .map_or("", |(_, build)| build),
    )
}

pub(crate) fn ask_anything_available_for_version(prerelease: &str, build: &str) -> bool {
    !prerelease.starts_with("dev.noask.") && !build.starts_with("build.noask.")
}

pub(crate) fn platform_capabilities() -> openless_core::PlatformCapabilities {
    let mut capabilities = openless_core::PlatformCapabilities::current();
    capabilities.supports_auto_update &= UPDATES_ENABLED;
    capabilities
}
