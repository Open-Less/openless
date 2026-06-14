//! Pure Android updater helpers (manifest URLs, version compare). Testable on all targets.

pub const MIRROR_BASE: &str = "https://fastgit.cc/https://github.com/appergb/openless";
pub const DIRECT_BASE: &str = "https://github.com/appergb/openless";

pub fn map_abi_to_arch(abi: &str) -> &'static str {
    match abi {
        "arm64-v8a" => "aarch64",
        "armeabi-v7a" => "armv7",
        "x86" => "i686",
        "x86_64" => "x86_64",
        _ => "aarch64",
    }
}

pub fn version_is_newer(remote: &str, current: &str) -> bool {
    fn parts(v: &str) -> Vec<u32> {
        v.split(|c| c == '.' || c == '-')
            .filter_map(|p| p.parse().ok())
            .collect()
    }
    let remote_parts = parts(remote);
    let current_parts = parts(current);
    let max = remote_parts.len().max(current_parts.len());
    for i in 0..max {
        let r = remote_parts.get(i).copied().unwrap_or(0);
        let c = current_parts.get(i).copied().unwrap_or(0);
        if r > c {
            return true;
        }
        if r < c {
            return false;
        }
    }
    false
}

pub fn stable_manifest_urls(arch: &str) -> Vec<String> {
    vec![
        format!("{MIRROR_BASE}/releases/latest/download/latest-android-{arch}-mirror.json"),
        format!("{DIRECT_BASE}/releases/latest/download/latest-android-{arch}.json"),
    ]
}

pub fn beta_manifest_urls(arch: &str, tag: &str) -> Vec<String> {
    vec![
        format!("{MIRROR_BASE}/releases/download/{tag}/latest-android-{arch}-beta-mirror.json"),
        format!("{DIRECT_BASE}/releases/download/{tag}/latest-android-{arch}-beta.json"),
    ]
}

/// Human-readable manifest fetch failure for UI tooltips.
pub fn format_manifest_error(status: u16, url: &str) -> String {
    if status == 404 {
        format!("更新清单不存在 (404): {url}")
    } else {
        format!("无法获取更新清单 (HTTP {status}): {url}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_newer_detects_patch_bump() {
        assert!(version_is_newer("1.3.9", "1.3.8"));
        assert!(!version_is_newer("1.3.8", "1.3.9"));
        assert!(!version_is_newer("1.3.8", "1.3.8"));
    }

    #[test]
    fn version_is_newer_handles_beta_suffix() {
        assert!(version_is_newer("1.3.8-1", "1.3.8"));
        assert!(!version_is_newer("1.3.8", "1.3.8-1"));
    }

    #[test]
    fn stable_manifest_urls_use_latest_download_path() {
        let urls = stable_manifest_urls("aarch64");
        assert_eq!(urls.len(), 2);
        assert!(urls[0].contains("latest-android-aarch64-mirror.json"));
        assert!(urls[1].ends_with("latest-android-aarch64.json"));
        assert!(!urls[1].contains("-beta"));
    }

    #[test]
    fn beta_manifest_urls_include_tag_and_beta_suffix() {
        let urls = beta_manifest_urls("aarch64", "v1.3.8-1-beta-tauri");
        assert!(urls[0].contains("/releases/download/v1.3.8-1-beta-tauri/"));
        assert!(urls[1].ends_with("latest-android-aarch64-beta.json"));
    }

    #[test]
    fn map_abi_to_arch_maps_arm64() {
        assert_eq!(map_abi_to_arch("arm64-v8a"), "aarch64");
    }
}
