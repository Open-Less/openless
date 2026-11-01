//! Linux 桌面包的更新检测。
//!
//! Tauri 版用 `tauri-plugin-updater` 直接下载安装（macOS 的 `.app.tar.gz`、Windows 的
//! NSIS、Android 的 APK）。Linux 只发 deb/rpm，没有可自替换的封装，所以这里只做到
//! **发现新版本 + 给出下载页**：判断依据是 GitHub Releases 的公开 atom 源（Tauri 的
//! Beta 检测用的是同一个源，CDN 缓存、没有 REST 的 60 次/小时限流）。
//!
//! 标签约定：`v<版本>-tauri`（例如 `v2.0.2-Beta.3-tauri`）。Linux 包挂在同一个 release
//! 上，所以不需要另开清单。


/// 发布页（与 About 页的「发布说明」同一个链接）。
pub const RELEASES_URL: &str = "https://github.com/Open-Less/openless/releases";
const RELEASES_ATOM: &str = "https://github.com/Open-Less/openless/releases.atom";
const CHECK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// 跟哪个频道：稳定版只看正式发布，测试版也接受 `-Beta.N`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateChannel {
    Stable,
    Beta,
}

impl UpdateChannel {
    pub fn is_beta(self) -> bool {
        matches!(self, Self::Beta)
    }
}

/// 一条发布记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub tag: String,
    /// 去掉 `v` 前缀与 `-tauri` 后缀后的版本号，例如 `2.0.2-Beta.3`。
    pub version: String,
    pub url: String,
}

/// 检查结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateCheck {
    /// 当前版本（来自构建期注入的 `OPENLESS_APP_VERSION`）。
    pub current: String,
    /// 频道里最新的那条发布。
    pub latest: Release,
    /// 最新版本是否比当前版本新。
    pub newer: bool,
}


/// 从 atom 里取发布列表（保持源顺序，GitHub 是新的在前）。
///
/// 标签从 `<id>tag:github.com,2008:Repository/<repo>/v2.0.2-Beta.3-tauri</id>` 里取
/// —— `<title>` 是 `OpenLess v2.0.2-Beta.3-tauri`，带产品名前缀，不能直接当标签用。
pub fn parse_releases(atom: &str) -> Vec<Release> {
    let mut releases = Vec::new();
    for entry in atom.split("<entry>").skip(1) {
        let entry = entry.split("</entry>").next().unwrap_or(entry);
        let Some(tag) = text_of(entry, "id")
            .and_then(|id| id.rsplit('/').next().map(str::to_string))
            .filter(|tag| tag.starts_with('v'))
        else {
            continue;
        };
        let Some(url) = tag_link_href(entry) else {
            continue;
        };
        let Some(version) = release_version(&tag) else {
            continue;
        };
        releases.push(Release {
            tag,
            version,
            url,
        });
    }
    releases
}

/// `v2.0.2-Beta.3-tauri` → `2.0.2-Beta.3`；不符合约定的标签返回 `None`。
pub fn release_version(tag: &str) -> Option<String> {
    let tag = tag.trim();
    let tag = tag.strip_prefix('v')?;
    let tag = tag.strip_suffix("-tauri")?;
    (!tag.is_empty() && tag.starts_with(|c: char| c.is_ascii_digit())).then(|| tag.to_string())
}

/// 频道里最新的那条发布：稳定版跳过任何带预发布标记的标签。
pub fn select(channel: UpdateChannel, releases: &[Release]) -> Option<&Release> {
    releases
        .iter()
        .find(|release| channel.is_beta() || !is_prerelease(&release.version))
}

fn is_prerelease(version: &str) -> bool {
    version.contains('-')
}

/// 版本号比较：`2.0.2-Beta.3-149` 这类构建后缀不参与比较。
///
/// 规则与语义保持一致：数字段逐位比较；同一基线上正式版 > 预发布版；
/// 预发布之间按名字与序号比较（`Beta.10` > `Beta.9`）。
pub fn is_newer(current: &str, candidate: &str) -> bool {
    match (parse_version(current), parse_version(candidate)) {
        (Some(current), Some(candidate)) => candidate > current,
        // 解析不出来时退回字符串比较，至少不会把「看起来一样」当成更新。
        _ => trimmed(current) != trimmed(candidate),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedVersion(Vec<u64>, Option<(String, u64)>);

impl Ord for ParsedVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        // 数字段按位比较，短的一方补 0（`2.0` 与 `2.0.0` 视为同一版本）。
        let digits = self.0.len().max(other.0.len());
        for index in 0..digits {
            let left = self.0.get(index).copied().unwrap_or(0);
            let right = other.0.get(index).copied().unwrap_or(0);
            match left.cmp(&right) {
                Ordering::Equal => {}
                order => return order,
            }
        }
        // 同一基线上正式版高于任何预发布版。
        match (&self.1, &other.1) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (Some(left), Some(right)) => left.cmp(right),
        }
    }
}

impl PartialOrd for ParsedVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

fn parse_version(raw: &str) -> Option<ParsedVersion> {
    let trimmed = trimmed(raw);
    let (base, pre) = match trimmed.split_once('-') {
        Some((base, pre)) => (base, Some(pre)),
        None => (trimmed.as_str(), None),
    };
    let numbers = base
        .split('.')
        .map(|part| part.parse::<u64>().ok())
        .collect::<Option<Vec<u64>>>()?;
    let pre = pre.and_then(|pre| {
        let (name, index) = pre.rsplit_once('.')?;
        let index = index.parse::<u64>().ok()?;
        Some((name.to_ascii_lowercase(), index))
    });
    Some(ParsedVersion(numbers, pre))
}

/// 去掉构建期追加的 `-<数字>`（`2.0.2-Beta.3-149` → `2.0.2-Beta.3`）。
/// 只有最后一段是纯数字且前面还有内容时才截断，`Beta.3` 这类序号不受影响。
fn trimmed(raw: &str) -> String {
    let raw = raw.trim();
    if let Some((head, tail)) = raw.rsplit_once('-') {
        if !head.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) {
            return head.to_string();
        }
    }
    raw.to_string()
}

/// 网络检查。`Ok(None)` = 频道里还没有任何发布。
pub async fn check(channel: UpdateChannel) -> Result<Option<UpdateCheck>, String> {
    let response = openless_core::net::send_with_retry(|| {
        openless_core::net::http()
            .get(RELEASES_ATOM)
            .timeout(CHECK_TIMEOUT)
    })
    .await
    .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("releases.atom: HTTP {}", response.status()));
    }
    let body = response.text().await.map_err(|error| error.to_string())?;
    let releases = parse_releases(&body);
    let Some(latest) = select(channel, &releases).cloned() else {
        return Ok(None);
    };
    let current = env!("OPENLESS_APP_VERSION").to_string();
    Ok(Some(UpdateCheck {
        newer: is_newer(&current, &latest.version),
        current,
        latest,
    }))
}

fn text_of(entry: &str, tag: &str) -> Option<String> {
    let start = entry.find(&format!("<{tag}>"))? + tag.len() + 2;
    let end = entry[start..].find(&format!("</{tag}>"))? + start;
    let value = entry[start..end].trim();
    (!value.is_empty()).then(|| unescape(value))
}

/// 取指向 release 页的那条链接（`rel="alternate"`），忽略 feed 级链接。
fn tag_link_href(entry: &str) -> Option<String> {
    let mut rest = entry;
    while let Some(start) = rest.find("<link") {
        rest = &rest[start..];
        let tag_end = rest.find('>')?;
        let element = &rest[..tag_end];
        if let Some(href_start) = element.find("href=\"") {
            let href = &element[href_start + "href=\"".len()..];
            if let Some(href_end) = href.find('"') {
                let value = &href[..href_end];
                if value.contains("/releases/tag/") {
                    return Some(value.to_string());
                }
            }
        }
        rest = &rest[tag_end..];
    }
    None
}

fn unescape(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ATOM: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <entry>
    <id>tag:github.com,2008:Repository/1222252809/v2.0.2-Beta.4-tauri</id>
    <link rel="alternate" type="text/html" href="https://github.com/Open-Less/openless/releases/tag/v2.0.2-Beta.4-tauri"/>
    <title>OpenLess v2.0.2-Beta.4-tauri</title>
  </entry>
  <entry>
    <id>tag:github.com,2008:Repository/1222252809/v2.0.2-Beta.3-tauri</id>
    <link rel="alternate" type="text/html" href="https://github.com/Open-Less/openless/releases/tag/v2.0.2-Beta.3-tauri"/>
    <title>OpenLess v2.0.2-Beta.3-tauri</title>
  </entry>
  <entry>
    <id>tag:github.com,2008:Repository/1222252809/v2.0.1-tauri</id>
    <link rel="alternate" type="text/html" href="https://github.com/Open-Less/openless/releases/tag/v2.0.1-tauri"/>
    <title>OpenLess v2.0.1-tauri</title>
  </entry>
  <entry>
    <id>tag:github.com,2008:Repository/1222252809/nightly-scratch</id>
    <link rel="alternate" type="text/html" href="https://example.invalid/x"/>
    <title>nightly-scratch</title>
  </entry>
</feed>"#;

    #[test]
    fn releases_parse_in_feed_order_and_skip_unrelated_tags() {
        let releases = parse_releases(ATOM);
        let versions: Vec<_> = releases.iter().map(|r| r.version.as_str()).collect();
        assert_eq!(versions, ["2.0.2-Beta.4", "2.0.2-Beta.3", "2.0.1"]);
        assert_eq!(
            releases[0].url,
            "https://github.com/Open-Less/openless/releases/tag/v2.0.2-Beta.4-tauri"
        );
    }

    #[test]
    fn stable_channel_skips_beta_tags() {
        let releases = parse_releases(ATOM);
        let stable = select(UpdateChannel::Stable, &releases).expect("stable release");
        assert_eq!(stable.version, "2.0.1");
        let beta = select(UpdateChannel::Beta, &releases).expect("beta release");
        assert_eq!(beta.version, "2.0.2-Beta.4");
    }

    #[test]
    fn version_comparison_ignores_the_build_revision() {
        // 构建后缀 `-149` 不参与比较：同一个 Beta.3 不算更新。
        assert!(!is_newer("2.0.2-Beta.3-149", "2.0.2-Beta.3"));
        assert!(is_newer("2.0.2-Beta.3-149", "2.0.2-Beta.4"));
        assert!(is_newer("2.0.2-Beta.3-149", "2.0.2"));
        assert!(!is_newer("2.0.2-Beta.3-149", "2.0.1"));
        // 序号按数字比较，不是字符串。
        assert!(is_newer("2.0.2-Beta.9", "2.0.2-Beta.10"));
        assert!(!is_newer("2.0.2", "2.0.2-Beta.4"));
    }

    /// 真源演练：需要网络，所以默认 `--ignored`。用来确认 GitHub 的 atom 结构
    /// 没变（标题带产品名前缀、标签在 `<id>` 里）。
    #[tokio::test]
    #[ignore = "requires network access to github.com"]
    async fn live_atom_yields_a_comparable_release() {
        let check = check(UpdateChannel::Beta).await.expect("live check");
        let check = check.expect("at least one published release");
        assert!(check.latest.version.starts_with("2."), "{check:?}");
        assert!(check.latest.url.contains("/releases/tag/"), "{check:?}");
        assert_eq!(check.current, env!("OPENLESS_APP_VERSION"));
    }

    #[test]
    fn missing_channel_release_is_not_an_error() {
        let empty = parse_releases("<feed></feed>");
        assert!(select(UpdateChannel::Stable, &empty).is_none());
    }
}
