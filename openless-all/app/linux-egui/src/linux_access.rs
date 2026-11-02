//! Read-only desktop session diagnostics. Never read secrets, unlock a wallet,
//! create a collection, or capture audio/text as part of a settings refresh.
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// 门槛：等用户解锁钥匙串的最长时间（系统密码框由 kwalletd / gcr 负责弹）。
pub const VAULT_UNLOCK_TIMEOUT: Duration = Duration::from_secs(120);

/// 钥匙串被锁定时的统一文案。
///
/// 以前「锁定」和「服务不可用」共用一句「无法检查默认保险库，请恢复原来的钱包」——
/// 用户拿这句话什么都做不了：钱包没丢，只是开机后还没解锁。锁定是可恢复状态，
/// 所以单独成句、标成 retryable，并且由 UI 提供「解锁钥匙串」按钮。
pub const VAULT_LOCKED_MESSAGE: &str =
    "The keyring is locked: unlock it in the system dialog and retry. Saved keys were not deleted.";

/// 解锁请求被用户取消或超时。
pub const VAULT_UNLOCK_CANCELLED_MESSAGE: &str =
    "The keyring is still locked (cancelled or timed out). Unlock it and retry; saved keys were not lost.";

pub fn vault_locked_error() -> openless_core::BackendError {
    openless_core::BackendError::new(
        openless_core::BackendErrorCode::Persistence,
        VAULT_LOCKED_MESSAGE,
    )
    .retryable(true)
}

fn vault_unlock_cancelled_error() -> openless_core::BackendError {
    openless_core::BackendError::new(
        openless_core::BackendErrorCode::Persistence,
        VAULT_UNLOCK_CANCELLED_MESSAGE,
    )
    .retryable(true)
}

fn vault_probe_error(message: &str) -> openless_core::BackendError {
    openless_core::BackendError::new(
        openless_core::BackendErrorCode::Persistence,
        message.to_string(),
    )
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckState {
    #[default]
    Unknown,
    Ready,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyringState {
    #[default]
    Unknown,
    Ready,
    Locked,
    MissingService,
    MissingCollection,
    /// A session-only default cannot safely hold persistent channel credentials.
    TemporaryVault,
    Unavailable,
    RestartRequired,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LinuxAccessSnapshot {
    pub checking: bool,
    pub keyring: KeyringState,
    pub input: CheckState,
    pub network: CheckState,
    pub network_latency_ms: Option<u64>,
}

/// Same bounded GET probe as Tauri's `check_network`. Any HTTP response proves
/// reachability to this OpenLess endpoint, NOT ASR/LLM authentication or health.
pub async fn probe_network() -> (CheckState, Option<u64>) {
    let start = Instant::now();
    let url = format!("{}/packs?limit=1", openless_core::MARKETPLACE_BASE_URL);
    match openless_core::net::http()
        .get(url)
        .timeout(Duration::from_secs(8))
        .send()
        .await
    {
        Ok(_) => (CheckState::Ready, Some(start.elapsed().as_millis() as u64)),
        Err(_) => (CheckState::Unavailable, None),
    }
}

#[cfg(target_os = "linux")]
pub fn probe_session() -> (KeyringState, CheckState) {
    use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
    use dbus::blocking::Connection;

    let Ok(connection) = Connection::new_session() else {
        return (KeyringState::Unavailable, CheckState::Unavailable);
    };
    let bus = connection.with_proxy(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        Duration::from_secs(1),
    );
    // Resolve unique owners so a disappeared service isn't auto-started by a
    // later properties/introspection query, and we don't mix two daemon owners.
    let owner = |name: &str| -> Result<String, dbus::Error> {
        let (owner,): (String,) =
            bus.method_call("org.freedesktop.DBus", "GetNameOwner", (name,))?;
        Ok(owner)
    };
    let keyring = match owner("org.freedesktop.secrets") {
        Err(error) if error.name() == Some("org.freedesktop.DBus.Error.NameHasNoOwner") => {
            KeyringState::MissingService
        }
        Err(_) => KeyringState::Unavailable,
        Ok(owner) => {
            let service = connection.with_proxy(
                owner.as_str(),
                "/org/freedesktop/secrets",
                Duration::from_secs(1),
            );
            let alias: Result<(dbus::Path<'static>,), _> =
                service.method_call("org.freedesktop.Secret.Service", "ReadAlias", ("default",));
            match alias {
                Err(_) => KeyringState::Unavailable,
                Ok((path,)) if path == "/" => KeyringState::MissingCollection,
                Ok((path,)) if temporary_collection(&path.to_string()) => {
                    KeyringState::TemporaryVault
                }
                Ok((path,)) => {
                    let collection =
                        connection.with_proxy(owner.as_str(), path, Duration::from_secs(1));
                    let locked: Result<bool, _> =
                        collection.get("org.freedesktop.Secret.Collection", "Locked");
                    match locked {
                        Err(_) => KeyringState::Unavailable,
                        Ok(true) => KeyringState::Locked,
                        // v1 caches initialization errors for the process lifetime.
                        // This checks only the adapter's session initialization;
                        // no Entry, secret read/write or Unlock call is involved.
                        //
                        // 但 `store_status()` 会**惰性初始化** zbus 存储（连接 + OpenSession），
                        // 那条路径没有超时。开机后守护进程还在启动时它会一直等，权限页就永远
                        // 「检查中」。放进看门狗线程里问，等不到就按「锁定」报，至少能引导用户
                        // 去解锁而不是干等。
                        Ok(false) => match credential_store_ready(Duration::from_secs(2)) {
                            StoreReady::Ready => KeyringState::Ready,
                            StoreReady::InitFailed => KeyringState::RestartRequired,
                            // 初始化卡住 = 有未应答的解锁请求，引导用户去解。
                            StoreReady::TimedOut => KeyringState::Locked,
                        },
                    }
                }
            }
        }
    };
    let input = owner(crate::fcitx5::DESTINATION)
        .ok()
        .and_then(|owner| {
            let addon =
                connection.with_proxy(owner, crate::fcitx5::OBJECT_PATH, Duration::from_secs(1));
            let (xml,): (String,) = addon
                .method_call("org.freedesktop.DBus.Introspectable", "Introspect", ())
                .ok()?;
            Some(input_interface_ready(&xml))
        })
        .unwrap_or(false);
    (
        keyring,
        if input {
            CheckState::Ready
        } else {
            CheckState::Unavailable
        },
    )
}

/// Secret Service 里「默认集合」的 D-Bus 路径，`None` = 还没配置默认集合。
#[cfg(target_os = "linux")]
fn default_collection_path(
    connection: &dbus::blocking::Connection,
) -> Result<dbus::Path<'static>, dbus::Error> {
    let bus = connection.with_proxy(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        Duration::from_secs(2),
    );
    let (owner,): (String,) = bus.method_call(
        "org.freedesktop.DBus",
        "GetNameOwner",
        ("org.freedesktop.secrets",),
    )?;
    let service = connection.with_proxy(owner, "/org/freedesktop/secrets", Duration::from_secs(2));
    let (path,): (dbus::Path<'static>,) =
        service.method_call("org.freedesktop.Secret.Service", "ReadAlias", ("default",))?;
    Ok(path)
}

/// 在**看门狗线程**里问一次 `keyring` 的存储是否初始化完了。
///
/// `keyring` v4 的 v1 兼容层会在第一次 `Entry::new` 时惰性初始化 zbus 存储
/// （连接 + `OpenSession`），全程没有超时。开机后守护进程还没准备好、或者有
/// 一个没人应答的解锁请求时，它就会一直挂着；调用方拿不到任何结果。
/// 所以这里把初始化丢到独立线程，最多等 `timeout`。
#[cfg(target_os = "linux")]
fn credential_store_ready(timeout: Duration) -> StoreReady {
    let (tx, rx) = std::sync::mpsc::channel();
    // 线程一旦挂住就不会回收，但 `store_status()` 的结果是进程级的 LazyLock，
    // 因此一个进程最多泄漏这一个线程。
    std::thread::Builder::new()
        .name("openless-vault-init".to_string())
        .spawn(move || {
            let ready = if keyring::Entry::store_status().is_err() {
                StoreReady::InitFailed
            } else {
                StoreReady::Ready
            };
            let _ = tx.send(ready);
        })
        .ok();
    rx.recv_timeout(timeout).unwrap_or(StoreReady::TimedOut)
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StoreReady {
    Ready,
    InitFailed,
    TimedOut,
}

/// 请系统把默认钥匙串解锁（会弹出 kwalletd / gcr 的密码框）。
///
/// 设计边界：
/// - **只在用户明确要求时调用**（设置页的「解锁钥匙串」按钮）。开机自启的进程
///   不该在用户没在电脑前时弹密码框。
/// - 只解锁**默认集合**，绝不新建集合、绝不换默认、绝不删除任何条目。
/// - 全程有上限：`VAULT_UNLOCK_TIMEOUT` 到点就返回「已取消或超时」，不挂死调用方。
/// - 已经解锁时立刻返回 `Ok(())`，不打扰用户。
#[cfg(target_os = "linux")]
pub fn unlock_default_vault(timeout: Duration) -> Result<(), openless_core::BackendError> {
    use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
    use dbus::blocking::Connection;

    let started = Instant::now();
    let connection = Connection::new_session().map_err(|error| {
        vault_probe_error(&format!(
            "cannot reach the session bus: {}",
            error.message().unwrap_or_default()
        ))
    })?;
    let path = default_collection_path(&connection).map_err(|error| {
        vault_probe_error(&format!(
            "cannot read the default keyring: {}",
            error.message().unwrap_or_default()
        ))
    })?;
    if path == "/" {
        return Err(vault_probe_error(
            "No default Secret Service collection is configured.",
        ));
    }
    let owner = {
        let bus = connection.with_proxy(
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            Duration::from_secs(2),
        );
        let (owner,): (String,) = bus
            .method_call(
                "org.freedesktop.DBus",
                "GetNameOwner",
                ("org.freedesktop.secrets",),
            )
            .map_err(|error| {
                vault_probe_error(&format!(
                    "Secret Service is not running: {}",
                    error.message().unwrap_or_default()
                ))
            })?;
        owner
    };
    let collection = connection.with_proxy(owner.as_str(), path.clone(), Duration::from_secs(5));
    let locked: bool = collection
        .get("org.freedesktop.Secret.Collection", "Locked")
        .map_err(|error| {
            vault_probe_error(&format!(
                "cannot read the vault lock state: {}",
                error.message().unwrap_or_default()
            ))
        })?;
    if !locked {
        return Ok(());
    }

    // 先订阅 Completed，再触发 Prompt：顺序反了会漏掉信号。
    let outcome: std::sync::Arc<std::sync::Mutex<Option<bool>>> =
        std::sync::Arc::new(std::sync::Mutex::new(None));
    let (_unlocked, prompt): (Vec<dbus::Path<'static>>, dbus::Path<'static>) = collection
        .method_call("org.freedesktop.Secret.Collection", "Unlock", ())
        .map_err(|error| {
            vault_probe_error(&format!(
                "the unlock request failed: {}",
                error.message().unwrap_or_default()
            ))
        })?;
    if prompt == "/" {
        // 服务直接处理完了，没有交互流程。
        let locked: bool = collection
            .get("org.freedesktop.Secret.Collection", "Locked")
            .unwrap_or(true);
        return if locked {
            Err(vault_unlock_cancelled_error())
        } else {
            Ok(())
        };
    }
    {
        let flag = std::sync::Arc::clone(&outcome);
        let rule =
            dbus::message::MatchRule::new_signal("org.freedesktop.Secret.Prompt", "Completed");
        connection
            .add_match::<std::collections::VecDeque<Box<dyn dbus::arg::RefArg>>, _>(
                rule,
                move |args: std::collections::VecDeque<Box<dyn dbus::arg::RefArg>>,
                      _connection: &Connection,
                      _message: &dbus::Message| {
                    // body: (bool dismissed, Variant<...> result)
                    let dismissed = args
                        .front()
                        .and_then(|value| value.as_i64())
                        .map(|value| value != 0)
                        .unwrap_or(false);
                    if let Ok(mut slot) = flag.lock() {
                        *slot = Some(dismissed);
                    }
                    false // 收到一次就摘掉这个匹配
                },
            )
            .map_err(|error| {
                vault_probe_error(&format!(
                    "cannot subscribe to the unlock result: {}",
                    error.message().unwrap_or_default()
                ))
            })?;
    }
    let prompt_proxy = connection.with_proxy(owner.as_str(), prompt, Duration::from_secs(5));
    let _: () = prompt_proxy
        .method_call("org.freedesktop.Secret.Prompt", "Prompt", ())
        .map_err(|error| {
            vault_probe_error(&format!(
                "cannot show the unlock dialog: {}",
                error.message().unwrap_or_default()
            ))
        })?;

    while started.elapsed() < timeout {
        let settled = outcome.lock().map(|slot| *slot).unwrap_or(Some(true));
        if settled.is_some() {
            break;
        }
        let _ = connection.process(Duration::from_millis(150));
    }

    let dismissed = outcome.lock().ok().and_then(|slot| *slot);
    let locked: bool = collection
        .get("org.freedesktop.Secret.Collection", "Locked")
        .unwrap_or(true);
    match (dismissed, locked) {
        (Some(_), false) => Ok(()),
        _ => Err(vault_unlock_cancelled_error()),
    }
}

#[cfg(not(target_os = "linux"))]
pub fn unlock_default_vault(_timeout: Duration) -> Result<(), openless_core::BackendError> {
    Err(vault_probe_error("This platform has no Secret Service."))
}

fn temporary_collection(path: &str) -> bool {
    path == "/org/freedesktop/secrets/collection/session"
}

/// Refuse to read/write persistent channel configuration through an ephemeral
/// default. Never unlock, create a collection, or choose a different daemon.
#[cfg(target_os = "linux")]
pub(crate) fn ensure_persistent_vault() -> Result<(), openless_core::BackendError> {
    use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
    use dbus::blocking::Connection;
    let inspect = || -> Result<KeyringState, dbus::Error> {
        let connection = Connection::new_session()?;
        let bus = connection.with_proxy(
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            Duration::from_secs(1),
        );
        let (owner,): (String,) = bus.method_call(
            "org.freedesktop.DBus",
            "GetNameOwner",
            ("org.freedesktop.secrets",),
        )?;
        let service = connection.with_proxy(
            owner.as_str(),
            "/org/freedesktop/secrets",
            Duration::from_secs(1),
        );
        let (path,): (dbus::Path<'static>,) =
            service.method_call("org.freedesktop.Secret.Service", "ReadAlias", ("default",))?;
        if path == "/" {
            return Ok(KeyringState::MissingCollection);
        }
        if temporary_collection(&path.to_string()) {
            return Ok(KeyringState::TemporaryVault);
        }
        let collection = connection.with_proxy(owner, path, Duration::from_secs(1));
        let locked: bool = collection.get("org.freedesktop.Secret.Collection", "Locked")?;
        Ok(if locked {
            KeyringState::Locked
        } else {
            KeyringState::Ready
        })
    };
    match inspect() {
        Ok(KeyringState::Ready) => Ok(()),
        // 锁定是**可恢复**状态：单独一类错误，UI 据此给出「解锁钥匙串」按钮。
        Ok(KeyringState::Locked) => Err(vault_locked_error()),
        Ok(KeyringState::TemporaryVault) => Err(openless_core::BackendError::new(openless_core::BackendErrorCode::Persistence,
            "The Secret Service default vault is temporary (session-only). Existing credentials may be in another wallet. Select the original persistent Secret Service vault and restart OpenLess; credentials have not been deleted or migrated.")),
        Ok(_) | Err(_) => Err(openless_core::BackendError::new(openless_core::BackendErrorCode::Persistence,
            "The session Secret Service default vault could not be inspected. Restore the original persistent vault and restart OpenLess; no credentials were changed.")),
    }
}

#[cfg(not(target_os = "linux"))]
pub fn probe_session() -> (KeyringState, CheckState) {
    (KeyringState::Unavailable, CheckState::Unavailable)
}

#[cfg(target_os = "linux")]
fn input_interface_ready(xml: &str) -> bool {
    // fcitx5 的 introspection XML 带 DOCTYPE（D-Bus 标准格式），roxmltree 默认
    // 拒绝 DTD，解析失败会让这里永远返回 false——「权限与数据」页就会把正常的
    // 插件报成「不可用」。必须显式允许 DTD。
    let Ok(document) = roxmltree::Document::parse_with_options(
        xml,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            ..Default::default()
        },
    ) else {
        return false;
    };
    document.descendants().any(|node| {
        node.has_tag_name("interface")
            && node.attribute("name") == Some(crate::fcitx5::INTERFACE)
            && ["CommitText", "GetSelectionText"].iter().all(|name| {
                node.children().any(|method| {
                    method.has_tag_name("method") && method.attribute("name") == Some(*name)
                })
            })
    })
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use crate::i18n::Lang;

    #[test]
    fn temporary_defaults_are_never_reported_as_persistent_vaults() {
        assert!(temporary_collection(
            "/org/freedesktop/secrets/collection/session"
        ));
        assert!(!temporary_collection(
            "/org/freedesktop/secrets/collection/kdewallet"
        ));
        assert!(!temporary_collection(
            "/org/freedesktop/secrets/collection/login"
        ));
    }

    #[test]
    fn a_dbus_peer_or_wrong_addon_is_not_a_text_service() {
        assert!(!input_interface_ready(
            "<node><interface name='org.freedesktop.DBus.Peer'/></node>"
        ));
        assert!(!input_interface_ready("<node><interface name='org.fcitx.Fcitx.OpenLess1'><method name='CommitText'/></interface></node>"));
        assert!(!input_interface_ready("invalid XML"));
        assert!(input_interface_ready("<node><interface name='org.fcitx.Fcitx.OpenLess1'><method name='CommitText'/><method name='GetSelectionText'/></interface></node>"));
    }

    /// fcitx5 的 introspection 回复自带 DOCTYPE。解析器默认拒绝 DTD，一旦这里
    /// 静默失败，「权限与数据」页就会把正常工作的插件报成「不可用」。
    #[test]
    fn the_standard_dbus_doctype_does_not_break_the_probe() {
        let xml = r#"<!DOCTYPE node PUBLIC "-//freedesktop//DTD D-BUS Object Introspection 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/introspect.dtd">
<node>
  <interface name="org.fcitx.Fcitx.OpenLess1">
    <method name="CommitText"><arg direction="in" type="s"/><arg direction="out" type="b"/></method>
    <method name="GetSelectionText"><arg direction="out" type="s"/></method>
    <signal name="DictationKeyEvent"><arg type="u"/><arg type="u"/><arg type="b"/></signal>
  </interface>
</node>"#;
        assert!(
            input_interface_ready(xml),
            "a DOCTYPE-prefixed reply from fcitx5 must still be understood"
        );
    }

    /// 锁定的钥匙串是可恢复状态：消息必须能被 UI 认出来并翻译，
    /// 而且标成 retryable —— 以前它和「保险库丢了」挤在同一句里，
    /// 用户被要求「恢复原来的钱包」，实际上只是没解锁。
    #[test]
    fn a_locked_vault_reports_a_distinct_retryable_error() {
        let error = super::vault_locked_error();
        assert_eq!(error.message, super::VAULT_LOCKED_MESSAGE);
        assert!(error.retryable);
        assert_ne!(error.message, super::VAULT_UNLOCK_CANCELLED_MESSAGE);
    }

    /// 这两句话在五种语言里都有译文；漏翻会让界面直接显示英文原句。
    #[test]
    fn the_locked_vault_messages_are_localized() {
        for lang in [Lang::ZhCn, Lang::ZhTw, Lang::En, Lang::Ja, Lang::Ko] {
            let locked = crate::provider_error_text(lang, super::VAULT_LOCKED_MESSAGE);
            let cancelled = crate::provider_error_text(lang, super::VAULT_UNLOCK_CANCELLED_MESSAGE);
            assert_ne!(locked, super::VAULT_LOCKED_MESSAGE, "{lang:?}");
            assert_ne!(cancelled, super::VAULT_UNLOCK_CANCELLED_MESSAGE, "{lang:?}");
        }
    }
}
