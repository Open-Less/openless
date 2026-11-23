pub(crate) fn classify_target_os(target: &str, cargo_cfg_target_os: Option<&str>) -> &'static str {
    match cargo_cfg_target_os {
        Some("android") => "android",
        Some("macos") => "macos",
        Some("linux") => "linux",
        Some(_) => "",
        None if target.contains("-android") || target.contains("-androideabi") => "android",
        None if target.ends_with("-apple-darwin") => "macos",
        None if target.contains("-linux-") => "linux",
        None => "",
    }
}

#[cfg(test)]
mod tests {
    use super::classify_target_os;

    #[test]
    fn cargo_target_os_classifies_android_triples() {
        for target in [
            "aarch64-linux-android",
            "armv7-linux-androideabi",
            "i686-linux-android",
            "x86_64-linux-android",
        ] {
            assert_eq!(classify_target_os(target, Some("android")), "android");
        }
    }

    #[test]
    fn cargo_target_os_classifies_desktop_targets() {
        assert_eq!(
            classify_target_os("x86_64-unknown-linux-gnu", Some("linux")),
            "linux"
        );
        assert_eq!(
            classify_target_os("aarch64-apple-darwin", Some("macos")),
            "macos"
        );
    }

    #[test]
    fn fallback_classifies_legacy_target_triples() {
        assert_eq!(
            classify_target_os("armv7-linux-androideabi", None),
            "android"
        );
        assert_eq!(
            classify_target_os("x86_64-unknown-linux-gnu", None),
            "linux"
        );
    }
}

#[cfg(test)]
mod capability_consistency {
    /// 宿主的 `cfg(local_asr_sherpa)` 由 build.rs 按目标注入，平台层用真 feature 表达。
    /// 两者必须一致，否则"引擎可用性"会在不同代码路径下给出不同答案。
    #[test]
    fn local_asr_sherpa_cfg_matches_the_platform_feature() {
        assert_eq!(
            cfg!(local_asr_sherpa),
            openless_platform::asr::SHERPA_AVAILABLE,
            "build.rs cfg(local_asr_sherpa) drifted from openless-platform's asr-sherpa feature"
        );
    }

    /// 同上：Foundry 的 WinML 运行时。
    #[test]
    fn foundry_runtime_cfg_matches_the_platform_feature() {
        assert_eq!(
            cfg!(all(target_os = "windows", local_asr_sherpa)),
            openless_platform::asr::FOUNDRY_AVAILABLE,
            "Windows Foundry runtime availability drifted from openless-platform's asr-foundry feature"
        );
    }
}
