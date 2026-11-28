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

pub(crate) fn platform_capabilities() -> openless_core::PlatformCapabilities {
    let mut capabilities = openless_core::PlatformCapabilities::current();
    capabilities.supports_auto_update &= UPDATES_ENABLED;
    capabilities
}
