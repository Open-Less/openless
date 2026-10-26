//! Where the in-app updater looks for release manifests.
//!
//! # Background
//!
//! The updater used to read its manifest from the GitHub Release assets
//! (`releases/latest/download/latest-{target}-{arch}.json`, and a `-beta` twin per tag). That
//! forced every release to carry fourteen `latest-*.json` files — seven target/arch entries
//! times a `-mirror` twin — all repeating the same three fields (`version`, `url`, `signature`),
//! and every new release re-uploaded the same file names because the Stable endpoint was a
//! rolling pointer.
//!
//! The manifests now live on the project's own CDN:
//!
//! ```text
//! https://dl.aeoform.com/updates/stable/latest-{target}-{arch}.json
//! https://dl.aeoform.com/updates/beta/latest-{target}-{arch}-beta.json
//! ```
//!
//! Two properties matter:
//!
//! - **One rolling file per platform.** The CDN name is stable per channel, so a release never
//!   accumulates manifest copies; publishing overwrites in place.
//! - **Channel separation is a path, not a filename suffix.** `/updates/stable/` and
//!   `/updates/beta/` are distinct locations, so a Stable client cannot walk into a Beta
//!   manifest by accident.
//!
//! The manifest body is unchanged (`version`, `pub_date`, `url`, `signature`), so
//! `tauri-plugin-updater` keeps owning download, minisign verification and platform install.
//! Artifact URLs inside the manifest point at the same CDN (`/releases/{tag}/{asset}`), which
//! is what mainland-China users download from.

use crate::types::UpdateChannel;

/// CDN that serves both the update manifests and the mirrored release artifacts.
pub(crate) const CDN_BASE: &str = "https://dl.aeoform.com";

/// Manifest URL for `channel`, with the `{{target}}` / `{{arch}}` tokens left in place for
/// `tauri-plugin-updater` to substitute at check time.
///
/// `format!` doubles the braces, so `{{{{target}}}}` renders as the literal `{{target}}`.
pub fn endpoint_template(channel: UpdateChannel) -> String {
    let (path, suffix) = match channel {
        UpdateChannel::Stable => ("stable", ""),
        UpdateChannel::Beta => ("beta", "-beta"),
    };
    format!("{CDN_BASE}/updates/{path}/latest-{{{{target}}}}-{{{{arch}}}}{suffix}.json")
}

/// Concrete manifest URL for clients that fetch the manifest themselves (Android).
///
/// `target`/`arch` use the updater token convention (`android` + `aarch64`, `darwin` +
/// `x86_64`, …), i.e. the same values the plugin would substitute into
/// [`endpoint_template`].
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub fn manifest_url(channel: UpdateChannel, target: &str, arch: &str) -> String {
    endpoint_template(channel)
        .replace("{{target}}", target)
        .replace("{{arch}}", arch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_endpoint_has_no_beta_marker() {
        let url = endpoint_template(UpdateChannel::Stable);
        assert!(
            !url.ends_with('/'),
            "endpoint must not end with a slash: {url}"
        );
        assert!(url.starts_with("https://dl.aeoform.com/updates/stable/"));
        assert!(url.contains("{{target}}") && url.contains("{{arch}}"));
        assert!(!url.contains("beta"));
    }

    #[test]
    fn beta_endpoint_is_a_separate_path_with_suffix() {
        let url = endpoint_template(UpdateChannel::Beta);
        assert!(url.starts_with("https://dl.aeoform.com/updates/beta/"));
        assert!(url.ends_with("-{{target}}-{{arch}}-beta.json"));
    }

    #[test]
    fn manifest_url_substitutes_every_token() {
        for (channel, expected) in [
            (
                UpdateChannel::Stable,
                "https://dl.aeoform.com/updates/stable/latest-darwin-aarch64.json",
            ),
            (
                UpdateChannel::Beta,
                "https://dl.aeoform.com/updates/beta/latest-darwin-aarch64-beta.json",
            ),
        ] {
            let url = manifest_url(channel, "darwin", "aarch64");
            assert_eq!(url, expected);
            assert!(!url.contains("{{"), "unsubstituted token in {url}");
        }
    }

    #[test]
    fn manifest_url_covers_every_published_target() {
        for (target, arch, expected) in [
            ("darwin", "aarch64", "latest-darwin-aarch64.json"),
            ("darwin", "x86_64", "latest-darwin-x86_64.json"),
            ("windows", "x86_64", "latest-windows-x86_64.json"),
            ("android", "aarch64", "latest-android-aarch64.json"),
            ("android", "armv7", "latest-android-armv7.json"),
            ("android", "x86_64", "latest-android-x86_64.json"),
            ("android", "i686", "latest-android-i686.json"),
        ] {
            assert!(
                manifest_url(UpdateChannel::Stable, target, arch).ends_with(expected),
                "{target}/{arch} should resolve to {expected}"
            );
            assert!(
                manifest_url(UpdateChannel::Beta, target, arch)
                    .ends_with(&expected.replace(".json", "-beta.json")),
                "{target}/{arch} beta should carry the -beta suffix"
            );
        }
    }

    #[test]
    fn cdn_base_has_no_trailing_slash() {
        assert!(!CDN_BASE.ends_with('/'));
        assert!(endpoint_template(UpdateChannel::Stable).starts_with(CDN_BASE));
    }
}
