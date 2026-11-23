pub fn open_external_url(url: &str) -> Result<(), String> {
    let parsed = url::Url::parse(url).map_err(|error| format!("invalid URL: {error}"))?;
    match parsed.scheme() {
        "http" | "https" => {}
        scheme => return Err(format!("unsupported URL scheme: {scheme}")),
    }

    platform_open_external_url(parsed.as_str())
}

#[cfg(target_os = "android")]
fn platform_open_external_url(url: &str) -> Result<(), String> {
    openless_platform::android::open_external_url(url)
}

#[cfg(not(target_os = "android"))]
fn platform_open_external_url(_url: &str) -> Result<(), String> {
    Err("native external URL fallback is only wired on Android".to_string())
}
