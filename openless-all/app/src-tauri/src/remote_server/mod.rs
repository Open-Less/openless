//! 远程输入（局域网手机录音）的 HTTPS + WebSocket 服务。
//!
//! 手机在同一局域网用浏览器打开 `https://<PC-IP>:<port>`，得到一个录音页
//! （assets/ 下的 index.html / app.js / style.css，编译期 include_str! 内嵌）。
//! 手机录音以 16k/单声道/16-bit LE PCM 经 WebSocket 实时推回 PC，由 Coordinator
//! 当作"手机麦克风"喂进现有「录音→ASR→润色→光标落字」管线（见
//! `Coordinator::start_remote_dictation`）。
//!
//! 关键约束：浏览器 `getUserMedia` 仅在安全上下文可用，所以必须 HTTPS。证书用
//! rcgen 自签名（SAN 含本机局域网 IP），手机首次访问需手动信任。TLS 走 ring
//! 后端（与项目 reqwest/tungstenite 一致，避免 aws-lc-sys 的 C 编译依赖）。

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::State,
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use hyper_util::rt::{TokioExecutor, TokioIo};
use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Listener, Manager};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

use crate::coordinator::Coordinator;

mod assets {
    pub const INDEX_HTML: &str = include_str!("assets/index.html");
    pub const APP_JS: &str = include_str!("assets/app.js");
    pub const STYLE_CSS: &str = include_str!("assets/style.css");
    pub const ICON_PNG: &[u8] = include_bytes!("assets/icon.png");
    pub const MIC_PNG: &[u8] = include_bytes!("assets/mic.png");
    pub const DONE_PNG: &[u8] = include_bytes!("assets/done.png");
}

const HEADER_HTML: &str = "text/html; charset=utf-8";
const HEADER_JS: &str = "application/javascript; charset=utf-8";
const HEADER_CSS: &str = "text/css; charset=utf-8";

/// 同一来源 IP 连续输错 PIN 的锁定阈值与时长。按 IP 而非全局计数：全局锁会被
/// 局域网内多台机器分摊（每台只贡献几次失败就触发全局锁，反而 DoS 正常用户）；
/// 按 IP 则每个攻击源各自被限到 ~5 次/分钟，10^6 个 PIN 组合在锁定节奏下不可行。
const PIN_MAX_FAILS: u32 = 5;
const PIN_LOCK_SECS: u64 = 60;
/// pin_fails 表的容量上限：超过即清理已过期/已解锁的条目，防止伪造海量源 IP 撑爆内存。
const PIN_FAILS_MAX_ENTRIES: usize = 256;
/// 全局（跨 IP）PIN 失败上限和重置窗口。防止局域网内多台设备轮换 IP 绕过按 IP 的限速。
/// 20 次/分钟 ≈ 0.02% 的 PIN 空间，在 60s 锁定前实际可试到的组合数极少。
const PIN_GLOBAL_MAX_FAILS: u32 = 20;
const PIN_GLOBAL_WINDOW_SECS: u64 = 60;
/// 单个 PCM 二进制帧的上限。16kHz/16bit 实时流正常每帧只有几 KB，64KB ≈ 2 秒音频；
/// 超限帧直接丢弃，防已配对客户端（或驱动它的恶意网页）推超大帧造成内存压力。
const MAX_PCM_FRAME_BYTES: usize = 64 * 1024;
/// 服务端 keepalive：每 KEEPALIVE_PING_SECS 发一次 WS Ping（浏览器自动回 Pong）；
/// 连续 IDLE_TIMEOUT_SECS 收不到任何上行帧（含 Pong）则视为半开死链断开。
/// 手机息屏/Wi-Fi 漂移常常不发 TCP FIN，没有探活时 recv() 永久挂起：连接任务、
/// 事件订阅、进行中的远程会话全部悬挂。
const KEEPALIVE_PING_SECS: u64 = 30;
const IDLE_TIMEOUT_SECS: u64 = 90;

// ───────────────────────── 对外类型 ─────────────────────────

pub struct RemoteServerConfig {
    pub port: u16,
    pub pin: String,
    pub coordinator: Arc<Coordinator>,
    pub app: AppHandle,
}

/// 运行中的服务句柄。drop / shutdown 触发优雅关停。
pub struct RemoteServerHandle {
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    /// 广播给所有已建立 WS 连接的关停信号。只停 accept loop 是不够的：连接任务
    /// 是独立 spawn 的，不通知它们的话，用户关掉远程输入（或重置 PIN 触发重启）
    /// 后已配对的手机会话原样存活，仍能录音、向 PC 光标落字——撤销语义失效。
    conn_shutdown_tx: tokio::sync::watch::Sender<bool>,
    join: tauri::async_runtime::JoinHandle<()>,
    pub bound_port: u16,
    #[allow(dead_code)]
    pub pin: String,
}

impl RemoteServerHandle {
    /// 通知 accept loop 与所有存量 WS 连接退出，并等待 accept loop 结束。
    pub async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        let _ = self.conn_shutdown_tx.send(true);
        let _ = self.join.await;
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteInputStatus {
    pub running: bool,
    pub port: u16,
    pub pin: String,
    pub urls: Vec<String>,
}

// ───────────────────────── 工具函数 ─────────────────────────

/// 生成 6 位数字配对码。使用 rejection sampling 消除取模偏差（u32::MAX 不是
/// 1_000_000 的倍数，直接取模会给低编号 PIN 带来约 0.03% 的额外概率）。
pub fn generate_pin() -> String {
    // u32::MAX + 1 = 2^32；舍弃使结果偏置的高端余数区间。
    // 偏置截止点：u32::MAX - (u32::MAX % 1_000_000) + 1（向下取整到 1_000_000 的倍数）
    const LIMIT: u32 = u32::MAX - (u32::MAX % 1_000_000);
    loop {
        let b = uuid::Uuid::new_v4().into_bytes();
        let n = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        if n < LIMIT {
            return format!("{:06}", n % 1_000_000);
        }
        // 极罕见（约 2^32 mod 10^6 / 2^32 ≈ 0.007% 概率），重新采样即可。
    }
}

fn pin_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("remote-input-pin.txt"))
}

/// 读持久化的配对码；没有 / 无效则新生成并写盘。让配对码跨重启稳定 —— 否则每次启动
/// 都重新随机一个，用户得反复回来找新码（"配对码错误"的根因）。
pub fn load_or_create_pin(app: &AppHandle) -> String {
    if let Some(p) = pin_path(app) {
        if let Ok(s) = std::fs::read_to_string(&p) {
            let s = s.trim();
            if s.len() == 6 && s.bytes().all(|b| b.is_ascii_digit()) {
                return s.to_string();
            }
        }
    }
    let pin = generate_pin();
    save_pin(app, &pin);
    pin
}

/// 写配对码到磁盘（用户点"重置配对码"时覆盖）。
pub fn save_pin(app: &AppHandle, pin: &str) {
    if let Some(p) = pin_path(app) {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&p, pin);
    }
}

fn is_private_lan(ip: &Ipv4Addr) -> bool {
    let o = ip.octets();
    !ip.is_loopback()
        && !ip.is_link_local()
        && ((o[0] == 192 && o[1] == 168)
            || o[0] == 10
            || (o[0] == 172 && (16..=31).contains(&o[1])))
}

/// 本机所有局域网 IPv4（过滤回环 / link-local / 虚拟网卡的非私网段）。
pub fn local_lan_ipv4s() -> Vec<Ipv4Addr> {
    let mut out: Vec<Ipv4Addr> = Vec::new();
    if let Ok(ifaces) = local_ip_address::list_afinet_netifas() {
        for (_name, ip) in ifaces {
            if let IpAddr::V4(v4) = ip {
                if is_private_lan(&v4) {
                    out.push(v4);
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// 给前端展示的访问网址列表。
pub fn access_urls(port: u16) -> Vec<String> {
    local_lan_ipv4s()
        .iter()
        .map(|ip| format!("https://{ip}:{port}"))
        .collect()
}

// ───────────────────────── TLS ─────────────────────────

/// 自签名证书：持久化到磁盘并跨重启复用。否则每次启动证书都变 —— 手机（尤其 iOS
/// Safari）上一次信任过的证书立刻失效，wss 握手静默挂起，表现为"连接中"卡死。仅当
/// 磁盘无证书 / 解析失败 / 当前局域网 IP 不在已存 SAN 列表里（换了网络）时才重新生成。
/// 返回 (证书 DER 原始字节, 私钥)。
fn load_or_generate_cert(
    dir: Option<&std::path::Path>,
    sans: &[String],
) -> Result<(Vec<u8>, rustls::pki_types::PrivateKeyDer<'static>), String> {
    use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
    // 文件名带 schema 版本：证书结构变更（v4 改回非 CA 服务器证书）时旧文件自动失效、重新生成。
    const CERT_FILE: &str = "remote-cert-v4.der";
    const KEY_FILE: &str = "remote-key-v4.der";
    const SANS_FILE: &str = "remote-cert-sans-v4.txt";
    if let Some(dir) = dir {
        if let (Ok(cert), Ok(key), Ok(saved)) = (
            std::fs::read(dir.join(CERT_FILE)),
            std::fs::read(dir.join(KEY_FILE)),
            std::fs::read_to_string(dir.join(SANS_FILE)),
        ) {
            let saved_set: std::collections::HashSet<&str> = saved.lines().collect();
            // 当前需要的 SAN 都在已存证书里 → 复用，证书保持稳定（手机信任一次长期有效）。
            if sans.iter().all(|s| saved_set.contains(s.as_str())) {
                log::info!("[remote-input] reusing persisted self-signed server cert");
                return Ok((cert, PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key))));
            }
        }
    }
    // 生成自签名服务器证书（SAN 含本机各局域网 IP）。主路径是浏览器页面级
    // “继续访问/访问此网站”例外；/cert.cer 与 /cert.mobileconfig 是手机系统级
    // 安装信任的兜底（部分浏览器的 wss 不复用页面级例外时使用）。
    let (cert_der, key_der) = {
        use rcgen::{
            CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, KeyPair,
            KeyUsagePurpose,
        };
        let mut params =
            CertificateParams::new(sans.to_vec()).map_err(|e| format!("rcgen params: {e}"))?;
        // 关键：做成普通服务器证书（非 CA，rcgen 默认即 NoCa）。iOS Safari 用页面级
        // “访问此网站”即可信任、无需安装证书 —— 这正是之前一直能用的方式。把证书做成 CA
        // 反而会让 iOS 拒绝页面级例外（CA 不能直接当服务器证书），导致一直超时。
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, "OpenLess Remote Input");
        dn.push(DnType::OrganizationName, "OpenLess");
        params.distinguished_name = dn;
        params.key_usages.push(KeyUsagePurpose::DigitalSignature);
        params
            .extended_key_usages
            .push(ExtendedKeyUsagePurpose::ServerAuth);
        let key_pair = KeyPair::generate().map_err(|e| format!("rcgen keypair: {e}"))?;
        let cert = params
            .self_signed(&key_pair)
            .map_err(|e| format!("rcgen self_signed: {e}"))?;
        (cert.der().as_ref().to_vec(), key_pair.serialize_der())
    };
    if let Some(dir) = dir {
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::write(dir.join(CERT_FILE), &cert_der);
        let _ = std::fs::write(dir.join(KEY_FILE), &key_der);
        // 私钥收紧为 0600：app 配置目录通常已是用户私有，但多用户/共享主机上
        // 默认 umask 可能给到组/其他用户可读。Windows 下 %APPDATA% 的 ACL
        // 本身仅限本用户，无对应权限位可设。
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(
                dir.join(KEY_FILE),
                std::fs::Permissions::from_mode(0o600),
            );
        }
        let _ = std::fs::write(dir.join(SANS_FILE), sans.join("\n"));
        log::info!("[remote-input] generated new self-signed server cert (SAN={sans:?})");
    }
    Ok((
        cert_der,
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_der)),
    ))
}

fn build_server_config(
    cert_der: Vec<u8>,
    key_der: rustls::pki_types::PrivateKeyDer<'static>,
) -> Result<Arc<rustls::ServerConfig>, String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| format!("tls protocol: {e}"))?
        .with_no_client_auth()
        .with_single_cert(
            vec![rustls::pki_types::CertificateDer::from(cert_der)],
            key_der,
        )
        .map_err(|e| format!("tls cert: {e}"))?;
    Ok(Arc::new(config))
}

// ───────────────────────── 启动 ─────────────────────────

struct WsState {
    pin: String,
    coordinator: Arc<Coordinator>,
    app: AppHandle,
    /// 按源 IP 的 PIN 失败计数 + 锁定截止时刻（防爆破；TLS+6 位 PIN 已是主防线）。
    pin_fails: Mutex<std::collections::HashMap<IpAddr, (u32, Option<Instant>)>>,
    /// 全局 PIN 失败计数 + 计数窗口起始时刻（防跨 IP 分布式暴力）。
    pin_global_fails: Mutex<(u32, Instant)>,
    /// 自签名证书的 DER 原始字节，供 /cert.cer 下载给手机安装信任。
    cert_der: Vec<u8>,
    /// 服务关停广播的接收端，每条 WS 连接 clone 一份并在主循环 select 监听。
    conn_shutdown_rx: tokio::sync::watch::Receiver<bool>,
}

/// 经 accept loop 注入的对端 IP（axum Extension）。hyper 直连 TLS 流时拿不到
/// ConnectInfo，这里在每条连接的 service 上挂一层 Extension 把 peer 传进 handler。
#[derive(Clone, Copy)]
struct PeerIp(IpAddr);

fn build_router(state: Arc<WsState>) -> Router {
    Router::new()
        .route("/", get(index_handler))
        .route(
            "/app.js",
            get(|| async {
                (
                    [(axum::http::header::CONTENT_TYPE, HEADER_JS)],
                    assets::APP_JS,
                )
            }),
        )
        .route(
            "/style.css",
            get(|| async {
                (
                    [(axum::http::header::CONTENT_TYPE, HEADER_CSS)],
                    assets::STYLE_CSS,
                )
            }),
        )
        .route(
            "/icon.png",
            get(|| async {
                (
                    [(axum::http::header::CONTENT_TYPE, "image/png")],
                    assets::ICON_PNG,
                )
            }),
        )
        .route(
            "/mic.png",
            get(|| async {
                (
                    [(axum::http::header::CONTENT_TYPE, "image/png")],
                    assets::MIC_PNG,
                )
            }),
        )
        .route(
            "/done.png",
            get(|| async {
                (
                    [(axum::http::header::CONTENT_TYPE, "image/png")],
                    assets::DONE_PNG,
                )
            }),
        )
        // 证书下载：手机在浏览器打开它即可下载并安装信任（iOS Safari 的 wss 不复用
        // 页面级证书例外，需在系统里完全信任后 wss 才稳定）。
        .route(
            "/cert.cer",
            get(|State(state): State<Arc<WsState>>| async move {
                (
                    [(
                        axum::http::header::CONTENT_TYPE,
                        "application/x-x509-ca-cert",
                    )],
                    state.cert_der.clone(),
                )
            }),
        )
        .route("/cert.mobileconfig", get(mobileconfig_handler))
        .route("/ws", get(ws_upgrade))
        .with_state(state)
}

/// 首页：按 PC 端当前界面语言把 `__OL_LANG__` 占位替换成实际 locale，
/// H5 据此（window.__OL_LANG__ / <html lang>）选择显示语言。
async fn index_handler(State(state): State<Arc<WsState>>) -> impl IntoResponse {
    let lang = state.coordinator.remote_locale();
    Html(assets::INDEX_HTML.replace("%%OL_LANG%%", &lang))
}

/// 极简标准 base64：构造 .mobileconfig 时把证书 DER 编码进 XML，避免引入额外依赖。
fn base64_encode(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        out.push(T[(b0 >> 2) as usize] as char);
        out.push(T[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        out.push(if chunk.len() > 1 {
            T[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[(b2 & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// iOS 配置描述文件：把证书包成 .mobileconfig。Safari 点击后凭 content-type
/// (application/x-apple-aspen-config) 直接进入“安装描述文件”流程，比裸 .cer 顺滑、
/// 也不会把当前页面导航走。安装后仍需到「设置→通用→关于本机→证书信任设置」打开完全信任。
///
/// 安全边界：PayloadType `com.apple.security.root` 只是 iOS 安装证书的固定入口，
/// 证书本身是非 CA 的纯服务器证书（rcgen NoCa + EKU=ServerAuth，见
/// load_or_generate_cert）——不含签发能力，无法用来给其他域名签证书做 MITM。
/// 信任它的影响范围仅限「持有本机私钥者可冒充 SAN 里列出的本机局域网 IP」，
/// 私钥只存在用户 PC 的应用配置目录。设置页 certTrustWarning 同步向用户说明。
async fn mobileconfig_handler(State(state): State<Arc<WsState>>) -> impl IntoResponse {
    let b64 = base64_encode(&state.cert_der);
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>PayloadContent</key><array><dict><key>PayloadCertificateFileName</key><string>openless.cer</string><key>PayloadContent</key><data>{b64}</data><key>PayloadType</key><string>com.apple.security.root</string><key>PayloadIdentifier</key><string>com.openless.remote-input.cert</string><key>PayloadUUID</key><string>A1B2C3D4-0001-4000-8000-000000000001</string><key>PayloadVersion</key><integer>1</integer><key>PayloadDisplayName</key><string>OpenLess Remote Input Certificate</string></dict></array><key>PayloadDisplayName</key><string>OpenLess Remote Input</string><key>PayloadIdentifier</key><string>com.openless.remote-input</string><key>PayloadType</key><string>Configuration</string><key>PayloadUUID</key><string>A1B2C3D4-0002-4000-8000-000000000002</string><key>PayloadVersion</key><integer>1</integer></dict></plist>"#,
        b64 = b64
    );
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "application/x-apple-aspen-config",
        )],
        xml,
    )
}

pub async fn start(cfg: RemoteServerConfig) -> Result<RemoteServerHandle, String> {
    let _ = HEADER_HTML; // index 用 axum Html() 自带 content-type
    let mut sans = vec!["localhost".to_string(), "127.0.0.1".to_string()];
    for ip in local_lan_ipv4s() {
        sans.push(ip.to_string());
    }
    // 证书目录用 app 配置目录（跨重启稳定）；拿不到则退回内存生成（不持久化）。
    let cert_dir = cfg.app.path().app_config_dir().ok();
    let (cert_der, key_der) = load_or_generate_cert(cert_dir.as_deref(), &sans)?;
    let rustls_config = build_server_config(cert_der.clone(), key_der)?;
    let acceptor = TlsAcceptor::from(rustls_config);

    let addr = SocketAddr::from(([0, 0, 0, 0], cfg.port));
    let listener = TcpListener::bind(addr).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::AddrInUse {
            "port-in-use".to_string()
        } else {
            format!("bind: {e}")
        }
    })?;
    let bound_port = listener.local_addr().map(|a| a.port()).unwrap_or(cfg.port);

    let (conn_shutdown_tx, conn_shutdown_rx) = tokio::sync::watch::channel(false);
    let state = Arc::new(WsState {
        pin: cfg.pin.clone(),
        coordinator: cfg.coordinator,
        app: cfg.app,
        pin_fails: Mutex::new(std::collections::HashMap::new()),
        pin_global_fails: Mutex::new((0, Instant::now())),
        cert_der,
        conn_shutdown_rx,
    });
    let router = build_router(state);

    let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let join = tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut shutdown_rx => {
                    log::info!("[remote-input] accept loop shutting down");
                    break;
                }
                accepted = listener.accept() => {
                    let (tcp, peer) = match accepted {
                        Ok(x) => x,
                        Err(e) => {
                            log::warn!("[remote-input] accept error: {e}");
                            continue;
                        }
                    };
                    // 最底层诊断：每个到达本机 8443 的 TCP 连接都记下来源 IP。手机一连就能
                    // 看到它到底有没有真的到这台电脑、来自哪个网段（排查"是不是连到别的设备"）。
                    log::info!("[remote-input] 收到 TCP 连接，来自 {peer}");
                    let acceptor = acceptor.clone();
                    // 每条连接把对端 IP 以 Extension 挂进 router，供 PIN 按 IP 锁定。
                    let router = router.clone().layer(axum::Extension(PeerIp(peer.ip())));
                    tokio::spawn(async move {
                        let tls = match acceptor.accept(tcp).await {
                            Ok(t) => t,
                            Err(e) => {
                                log::warn!("[remote-input] 来自 {peer} 的 TLS 握手失败（证书没被接受）：{e}");
                                return;
                            }
                        };
                        let io = TokioIo::new(tls);
                        let svc = hyper_util::service::TowerToHyperService::new(router);
                        let _ = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                            .serve_connection_with_upgrades(io, svc)
                            .await;
                    });
                }
            }
        }
    });

    Ok(RemoteServerHandle {
        shutdown_tx: Some(shutdown_tx),
        conn_shutdown_tx,
        join,
        bound_port,
        pin: cfg.pin,
    })
}

// ───────────────────────── WebSocket ─────────────────────────

async fn ws_upgrade(
    State(state): State<Arc<WsState>>,
    axum::Extension(PeerIp(peer_ip)): axum::Extension<PeerIp>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    // 能走到这里说明 wss 的 TLS 握手已成功（证书被手机接受）。排查"连不上"时看有没有
    // 这行：没有 = 卡在 TLS/证书（握手就失败）；有 = 握手 OK，问题在认证/后续逻辑。
    log::info!("[remote-input] WS 已升级：手机已通过 wss 接入（TLS/证书 OK）");
    ws.on_upgrade(move |socket| handle_ws(socket, state, peer_ip))
}

fn send_json<T: Serialize>(value: &T) -> Message {
    Message::Text(serde_json::to_string(value).unwrap_or_else(|_| "{}".into()))
}

/// 把后端 capsule 事件 payload 映射成手机端 status / level JSON 文本。
fn capsule_payload_to_phone(payload: &str) -> Vec<String> {
    let v: serde_json::Value = match serde_json::from_str(payload) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let state = v.get("state").and_then(|s| s.as_str()).unwrap_or("");
    let kind = match state {
        s if s.eq_ignore_ascii_case("recording") => "recording",
        s if s.eq_ignore_ascii_case("transcribing") => "transcribing",
        s if s.eq_ignore_ascii_case("polishing") => "polishing",
        s if s.eq_ignore_ascii_case("done") => "done",
        s if s.eq_ignore_ascii_case("error") => "error",
        s if s.eq_ignore_ascii_case("cancelled") => "done",
        _ => "",
    };
    let mut out = Vec::new();
    if !kind.is_empty() {
        let inserted = v
            .get("insertedChars")
            .or_else(|| v.get("inserted_chars"))
            .and_then(|n| n.as_u64());
        let message = v.get("message").and_then(|m| m.as_str());
        out.push(
            serde_json::json!({
                "type": "status",
                "kind": kind,
                "insertedChars": inserted,
                "message": message,
            })
            .to_string(),
        );
    }
    if let Some(level) = v.get("level").and_then(|l| l.as_f64()) {
        if state.eq_ignore_ascii_case("recording") {
            out.push(serde_json::json!({"type": "level", "value": level}).to_string());
        }
    }
    out
}

async fn handle_ws(mut socket: WebSocket, state: Arc<WsState>, peer_ip: IpAddr) {
    // 1) 握手：等第一帧 hello + PIN。
    let authed = match tokio::time::timeout(Duration::from_secs(15), socket.recv()).await {
        Ok(Some(Ok(Message::Text(txt)))) => verify_hello(&txt, &state, peer_ip),
        _ => return, // 超时 / 非文本首帧 / 断开
    };
    match authed {
        AuthResult::Ok => {
            log::info!("[remote-input] 配对成功，进入录音会话");
            let _ = socket
                .send(send_json(&serde_json::json!({"type":"auth","ok":true})))
                .await;
        }
        AuthResult::BadPin => {
            log::warn!("[remote-input] 配对码错误，已拒绝");
            let _ = socket
                .send(send_json(
                    &serde_json::json!({"type":"auth","ok":false,"reason":"bad-pin"}),
                ))
                .await;
            return;
        }
        AuthResult::Locked => {
            log::warn!("[remote-input] 配对已锁定（连续错误过多），已拒绝");
            let _ = socket
                .send(send_json(
                    &serde_json::json!({"type":"auth","ok":false,"reason":"locked"}),
                ))
                .await;
            return;
        }
    }

    // 2) 订阅 capsule 事件，转发给手机做状态显示。
    let (evt_tx, mut evt_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let listener_id = {
        let tx = evt_tx.clone();
        // 必须用 listen_any:capsule 状态是通过 emit_to("capsule", …) 定向发给胶囊
        // 窗口的,普通 app.listen(target=App) 收不到定向事件 —— 那样手机永远收不到
        // done/polishing 等状态,会一直卡在前端本地设的"识别中"。listen_any 接收
        // 所有 target 的事件,把胶囊状态如实转发给手机。
        state.app.listen_any("capsule:state", move |event| {
            for msg in capsule_payload_to_phone(event.payload()) {
                let _ = tx.send(msg);
            }
        })
    };

    // 听写完成后 PC 端把最终文字 emit 到 "remote:result"。手机用户看不到电脑屏幕,
    // 所以把这次落下的完整文字转发过去,H5 在状态区下方显示(type=result)。
    let result_listener_id = {
        let tx = evt_tx.clone();
        state.app.listen_any("remote:result", move |event| {
            // emit 的是 String,payload 是带引号的 JSON 字符串,反序列化回纯文本。
            if let Ok(text) = serde_json::from_str::<String>(event.payload()) {
                let _ = tx.send(serde_json::json!({ "type": "result", "text": text }).to_string());
            }
        })
    };

    // 3) 主循环：手机上行（控制 / PCM） + 后端状态下行 + keepalive 探活 + 关停广播。
    let mut conn_shutdown_rx = state.conn_shutdown_rx.clone();
    let mut keepalive = tokio::time::interval(Duration::from_secs(KEEPALIVE_PING_SECS));
    keepalive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_rx = Instant::now();
    loop {
        tokio::select! {
            incoming = socket.recv() => {
                last_rx = Instant::now();
                match incoming {
                    Some(Ok(Message::Binary(pcm))) => {
                        if pcm.len() >= 2 && pcm.len() % 2 == 0 && pcm.len() <= MAX_PCM_FRAME_BYTES {
                            state.coordinator.feed_remote_pcm(&pcm);
                        }
                    }
                    Some(Ok(Message::Text(txt))) => {
                        if !handle_control(&txt, &state, &mut socket).await {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => {}
                }
            }
            Some(msg) = evt_rx.recv() => {
                if socket.send(Message::Text(msg)).await.is_err() {
                    break;
                }
            }
            _ = keepalive.tick() => {
                // 半开探活：浏览器收到 Ping 自动回 Pong（上面 recv 收到即刷新 last_rx）。
                // 超时无任何上行 → 死链，break 走下方统一收尾（cancel + unlisten），
                // 避免录音中掉线时远程会话与标志悬挂。
                if last_rx.elapsed() > Duration::from_secs(IDLE_TIMEOUT_SECS) {
                    log::info!("[remote-input] 连接 {}s 无上行（含 Pong），按半开死链断开", IDLE_TIMEOUT_SECS);
                    break;
                }
                if socket.send(Message::Ping(Vec::new())).await.is_err() {
                    break;
                }
            }
            changed = conn_shutdown_rx.changed() => {
                // 服务关停（用户关闭远程输入 / 重置 PIN / 改端口触发重启）：
                // 主动断开存量连接，撤销已配对手机的会话与落字能力。
                if changed.is_err() || *conn_shutdown_rx.borrow() {
                    log::info!("[remote-input] 服务关停，断开存量手机连接");
                    break;
                }
            }
        }
    }

    // 4) 收尾：断连即取消未完成的远程会话，避免 ASR 句柄悬挂。
    log::info!("[remote-input] WS 连接已关闭");
    state.app.unlisten(listener_id);
    state.app.unlisten(result_listener_id);
    state.coordinator.cancel_remote_dictation();
}

/// 返回 false 表示应断开连接。
async fn handle_control(txt: &str, state: &Arc<WsState>, socket: &mut WebSocket) -> bool {
    let v: serde_json::Value = match serde_json::from_str(txt) {
        Ok(v) => v,
        Err(_) => return true,
    };
    match v.get("type").and_then(|t| t.as_str()).unwrap_or("") {
        "start" => {
            log::info!("[remote-input] 收到「开始录音」");
            match state.coordinator.start_remote_dictation().await {
                Ok(()) => {}
                Err(reason) => {
                    log::warn!("[remote-input] 开始录音被拒：{reason}");
                    let _ = socket
                        .send(send_json(
                            &serde_json::json!({"type":"busy","reason":reason}),
                        ))
                        .await;
                }
            }
        }
        "stop" => {
            log::info!("[remote-input] 收到「结束录音」");
            let _ = state.coordinator.stop_remote_dictation().await;
        }
        "cancel" => {
            state.coordinator.cancel_remote_dictation();
        }
        "set_insert" => {
            // 手机端「电脑落字」开关：value=true 表示要落字。no_insert = !value。
            let insert = v.get("value").and_then(|b| b.as_bool()).unwrap_or(true);
            state.coordinator.set_remote_no_insert(!insert);
            log::info!("[remote-input] 电脑落字开关 = {insert}");
        }
        _ => {}
    }
    true
}

enum AuthResult {
    Ok,
    BadPin,
    Locked,
}

fn verify_hello(txt: &str, state: &Arc<WsState>, peer_ip: IpAddr) -> AuthResult {
    // PIN 比较在锁外完成（无共享状态；constant_time_eq 防计时侧信道）。
    let v: serde_json::Value = match serde_json::from_str(txt) {
        Ok(v) => v,
        Err(_) => serde_json::Value::Null, // 非法 JSON 按 BadPin 计数
    };
    let pin_ok = v.get("type").and_then(|t| t.as_str()) == Some("hello")
        && v.get("pin")
            .and_then(|p| p.as_str())
            .map(|p| constant_time_eq(p.as_bytes(), state.pin.as_bytes()))
            .unwrap_or(false);

    // 锁定检查与失败累计放同一临界区：之前分两次拿锁，同一 IP 的并发握手可以
    // 都先通过锁定检查再各自累计失败，让计数越过阈值却不触发锁定。
    let now = Instant::now();

    // 全局限速：防局域网内多台设备轮换 IP 绕过按 IP 的限速（分布式暴力）。
    {
        let mut global = state.pin_global_fails.lock();
        let window_elapsed = now.duration_since(global.1).as_secs();
        if window_elapsed >= PIN_GLOBAL_WINDOW_SECS {
            // 滑动窗口到期，重置计数
            *global = (0, now);
        }
        if !pin_ok {
            global.0 += 1;
        }
        if global.0 >= PIN_GLOBAL_MAX_FAILS {
            return AuthResult::Locked;
        }
    }

    let mut guard = state.pin_fails.lock();
    if let Some((_, Some(until))) = guard.get(&peer_ip) {
        if now < *until {
            return AuthResult::Locked;
        }
        // 锁定到期，重置该 IP
        guard.remove(&peer_ip);
    }
    if pin_ok {
        guard.remove(&peer_ip);
        // 成功认证后重置全局计数，避免暴力后期合法用户被误锁
        let mut global = state.pin_global_fails.lock();
        global.0 = 0;
        AuthResult::Ok
    } else {
        // 容量兜底：先丢已解锁/过期的条目，防伪造海量源 IP 撑爆表。
        if guard.len() >= PIN_FAILS_MAX_ENTRIES {
            guard.retain(|_, (_, until)| matches!(until, Some(t) if *t > now));
        }
        let entry = guard.entry(peer_ip).or_insert((0, None));
        entry.0 += 1;
        if entry.0 >= PIN_MAX_FAILS {
            entry.1 = Some(now + Duration::from_secs(PIN_LOCK_SECS));
        }
        AuthResult::BadPin
    }
}

/// 等长常量时间比较，避免 PIN 计时侧信道。
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}
