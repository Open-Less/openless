//! 共享 HTTP 客户端 + 带重试的请求发送。
//!
//! 背景：原先每个网络命令各自 `reqwest::Client::new()`，连接池互不复用 —— 一次
//! 成功的 TLS 连接用完即弃，下一个命令又得重新握手。在握手不稳定的网络下（代理
//! 分流等）首次握手经常被重置，用户得反复重试才能用。
//!
//! 这里提供两件东西：
//! - `http()`：进程级共享客户端。一次握手成功后的连接进连接池，后续命令直接复用，
//!   不再付握手成本。
//! - `send_with_retry`：只对**连接层失败**（`is_connect()` —— 握手重置 / 连接被拒
//!   等）做指数退避重试。这类失败发生在请求送达服务端之前、且通常是瞬时的（代理
//!   分流抖动等），重试既幂等安全又有意义。**不重试超时与其他请求层错误**：超时
//!   可能发生在服务端已收到之后（重试 POST / DELETE 会重复执行）；`is_request()`
//!   类错误多为确定性失败（如 endpoint 配置错误），重试只是徒增数秒延迟。HTTP
//!   4xx/5xx 同样不重试 —— 服务端已应答，状态码交给调用方判断。

use std::collections::HashMap;
use std::time::Duration;

use once_cell::sync::Lazy;
use parking_lot::Mutex;

static HTTP: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        // 握手单独限时：卡在握手上要尽快失败，好让 send_with_retry 立即重试。
        .connect_timeout(Duration::from_secs(8))
        // 连接池：一条握手成功的连接保留 90s 供后续命令复用。
        .pool_idle_timeout(Duration::from_secs(90))
        .pool_max_idle_per_host(8)
        .tcp_keepalive(Duration::from_secs(30))
        .user_agent(concat!("OpenLess/", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
});

/// 进程级共享 HTTP 客户端。带连接池 —— 一次握手成功后的连接被后续请求复用。
pub fn http() -> &'static reqwest::Client {
    &HTTP
}

/// 按 `(timeout_secs, no_proxy)` 缓存并复用 `reqwest::Client`。
///
/// LLM / ASR provider 过去每次请求都新建一个 `reqwest::Client`，新客户端连接池是
/// 空的 —— 于是每句话都要重新 TLS 握手（~100–300ms）。这里把建好的客户端按其配置
/// 缓存：相同配置的后续 provider 直接 `clone()` 复用同一连接池（`reqwest::Client`
/// 内部是 `Arc`，clone 共享连接池与配置），握手成本只在首次付一次。
///
/// `build` 只在首次 miss 时调用，必须产出与该 `key` 语义一致的客户端。
pub fn cached_client<F>(key: (u64, bool), build: F) -> reqwest::Client
where
    F: FnOnce() -> reqwest::Client,
{
    static CACHE: Lazy<Mutex<HashMap<(u64, bool), reqwest::Client>>> =
        Lazy::new(|| Mutex::new(HashMap::new()));
    CACHE.lock().entry(key).or_insert_with(build).clone()
}

/// 单次请求最多尝试的次数。失败本身很快（握手重置 ~0.5s），10 次总耗时仍可控。
const MAX_ATTEMPTS: u32 = 10;

/// 发送请求，只对连接层失败（`is_connect()`：握手重置 / 连接被拒等）做指数退避重试。
///
/// `make` 每次尝试都重新构造 `RequestBuilder`（`send()` 会消耗它）。只重试
/// `is_connect()` —— 连接尚未建立、请求未送达服务端，且这类失败通常是瞬时的，
/// 重试幂等安全且有价值。超时（可能服务端已在处理）与其他 `is_request()` 类错误
/// （多为 endpoint 配置错误等确定性失败）都不重试。拿到任意 HTTP 响应（含
/// 4xx/5xx）即返回，状态码由调用方自行判断。
pub async fn send_with_retry<F>(make: F) -> reqwest::Result<reqwest::Response>
where
    F: Fn() -> reqwest::RequestBuilder,
{
    let mut attempt: u32 = 0;
    loop {
        attempt += 1;
        match make().send().await {
            Ok(resp) => return Ok(resp),
            Err(err) => {
                let retryable = err.is_connect();
                if !retryable || attempt >= MAX_ATTEMPTS {
                    return Err(err);
                }
                // 150 / 300 / 600 / 900 / 900 … ms 退避。
                let backoff = (150u64 * 2u64.pow((attempt - 1).min(3))).min(900);
                log::warn!(
                    "[net] transient failure (attempt {attempt}/{MAX_ATTEMPTS}), retry in {backoff}ms: {err}"
                );
                tokio::time::sleep(Duration::from_millis(backoff)).await;
            }
        }
    }
}
