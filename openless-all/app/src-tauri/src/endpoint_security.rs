//! Shared validation for user-configurable HTTP endpoints.
//!
//! Provider validation and the real request path must call the same policy;
//! otherwise a saved endpoint can bypass the checks performed by the
//! "validate connection" button.

use std::net::IpAddr;

pub(crate) struct ResolvedEndpoint {
    pub(crate) host: String,
    pub(crate) addrs: Vec<std::net::SocketAddr>,
}

pub(crate) fn validate_http_endpoint(raw: &str) -> anyhow::Result<()> {
    let url = url::Url::parse(raw).map_err(|e| anyhow::anyhow!("endpoint 不是合法 URL：{e}"))?;
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("endpoint 缺少主机名"))?
        .to_ascii_lowercase();

    const METADATA_HOSTS: [&str; 2] = ["metadata.google.internal", "169.254.169.254"];
    if METADATA_HOSTS
        .iter()
        .any(|metadata| host.contains(metadata))
    {
        anyhow::bail!("endpoint 指向云元数据服务，已拒绝：{host}");
    }

    let scheme = url.scheme();
    let bare_host = host
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(host.as_str());

    let Ok(ip) = bare_host.parse::<IpAddr>() else {
        if bare_host == "localhost" {
            return Ok(());
        }
        if scheme != "https" {
            anyhow::bail!("endpoint 必须使用 https（仅 localhost / 局域网允许 http）：{raw}");
        }
        return Ok(());
    };

    let canonical = match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(ip),
        v4 => v4,
    };

    let is_lan = match canonical {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private(),
        IpAddr::V6(v6) => v6.is_loopback() || (v6.segments()[0] & 0xfe00) == 0xfc00,
    };
    if is_lan {
        return Ok(());
    }

    let is_blocked = match canonical {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            let is_cgnat = octets[0] == 100 && (64..=127).contains(&octets[1]);
            v4.is_link_local() || v4.is_unspecified() || v4.is_broadcast() || is_cgnat
        }
        IpAddr::V6(v6) => {
            let is_link_local = (v6.segments()[0] & 0xffc0) == 0xfe80;
            v6.is_unspecified() || is_link_local
        }
    };
    if is_blocked {
        anyhow::bail!("endpoint 指向保留/危险地址，已拒绝（防 SSRF）：{ip}");
    }

    if scheme != "https" {
        anyhow::bail!("endpoint 必须使用 https（仅 localhost / 局域网允许 http）：{raw}");
    }

    Ok(())
}

/// Resolve a hostname once, validate every result, and return the addresses so
/// the HTTP client can pin this exact resolution and avoid DNS rebinding.
pub(crate) async fn resolve_http_endpoint(raw: &str) -> anyhow::Result<Option<ResolvedEndpoint>> {
    validate_http_endpoint(raw)?;
    let url = url::Url::parse(raw)?;
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("endpoint 缺少主机名"))?;
    if host.parse::<IpAddr>().is_ok() {
        return Ok(None);
    }
    let port = url
        .port_or_known_default()
        .ok_or_else(|| anyhow::anyhow!("endpoint 缺少端口"))?;
    let addrs: Vec<_> = tokio::net::lookup_host((host, port)).await?.collect();
    if addrs.is_empty() {
        anyhow::bail!("endpoint 主机名无法解析：{host}");
    }
    for addr in &addrs {
        let host = match addr.ip() {
            IpAddr::V4(ip) => ip.to_string(),
            IpAddr::V6(ip) => format!("[{ip}]"),
        };
        validate_http_endpoint(&format!("{}://{host}", url.scheme()))?;
    }
    Ok(Some(ResolvedEndpoint {
        host: host.to_string(),
        addrs,
    }))
}
