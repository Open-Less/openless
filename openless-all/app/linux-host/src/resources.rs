use std::path::{Path, PathBuf};

use openless_core::{BackendError, BackendErrorCode, DirectoryResourceResolver, ResourceResolver};

pub const FCITX_PLUGIN_LIBRARY: &str = "linux-fcitx5-plugin/libopenless.so";
pub const FCITX_PLUGIN_CONFIG: &str = "linux-fcitx5-plugin/openless.conf";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxPackageKind {
    Development,
    AppImage,
    SystemPackage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxResourceLayout {
    pub package_kind: LinuxPackageKind,
    pub resource_root: PathBuf,
}

impl LinuxResourceLayout {
    pub fn from_paths(
        executable: &Path,
        app_dir: Option<&Path>,
        explicit_resource_root: Option<&Path>,
    ) -> Result<Self, BackendError> {
        if let Some(root) = explicit_resource_root {
            return Ok(Self {
                package_kind: LinuxPackageKind::Development,
                resource_root: root.to_path_buf(),
            });
        }
        if let Some(app_dir) = app_dir {
            return Ok(Self {
                package_kind: LinuxPackageKind::AppImage,
                resource_root: app_dir.join("usr/lib/openless/resources"),
            });
        }
        let executable = executable.parent().ok_or_else(|| {
            BackendError::new(
                BackendErrorCode::Platform,
                "Linux executable has no parent directory",
            )
        })?;
        let system_root = executable.join("../lib/openless/resources");
        Ok(Self {
            package_kind: LinuxPackageKind::SystemPackage,
            resource_root: system_root,
        })
    }

    pub fn detect(explicit_resource_root: Option<PathBuf>) -> Result<Self, BackendError> {
        let executable = std::env::current_exe().map_err(|error| {
            BackendError::new(
                BackendErrorCode::Platform,
                format!("failed to resolve Linux executable: {error}"),
            )
        })?;
        let app_dir = std::env::var_os("APPDIR").map(PathBuf::from);
        Self::from_paths(
            &executable,
            app_dir.as_deref(),
            explicit_resource_root.as_deref(),
        )
    }

    pub fn resolver(&self) -> Result<LinuxResourceResolver, BackendError> {
        LinuxResourceResolver::new(self.resource_root.clone())
    }
}

#[derive(Debug, Clone)]
pub struct LinuxResourceResolver(DirectoryResourceResolver);

impl LinuxResourceResolver {
    pub fn new(root: PathBuf) -> Result<Self, BackendError> {
        DirectoryResourceResolver::new(root).map(Self)
    }

    pub fn root(&self) -> &Path {
        self.0.root()
    }
}

impl ResourceResolver for LinuxResourceResolver {
    fn resolve(&self, relative: &Path) -> Result<PathBuf, BackendError> {
        self.0.resolve(relative)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_layouts_keep_the_fcitx_contract_stable() {
        let appimage = LinuxResourceLayout::from_paths(
            Path::new("/tmp/.mount-openless/usr/bin/openless"),
            Some(Path::new("/tmp/.mount-openless")),
            None,
        )
        .unwrap();
        assert_eq!(
            appimage
                .resolver()
                .unwrap()
                .resolve(Path::new(FCITX_PLUGIN_LIBRARY))
                .unwrap(),
            PathBuf::from(
                "/tmp/.mount-openless/usr/lib/openless/resources/linux-fcitx5-plugin/libopenless.so"
            )
        );

        let system =
            LinuxResourceLayout::from_paths(Path::new("/usr/bin/openless"), None, None).unwrap();
        assert_eq!(system.package_kind, LinuxPackageKind::SystemPackage);
        assert!(system
            .resource_root
            .ends_with("bin/../lib/openless/resources"));
    }
}
