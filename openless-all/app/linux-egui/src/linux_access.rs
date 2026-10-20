//! Read-only desktop session diagnostics. Never read secrets, unlock a wallet,
//! create a collection, or capture audio/text as part of a settings refresh.
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

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
                        Ok(false) if keyring::Entry::store_status().is_err() => {
                            KeyringState::RestartRequired
                        }
                        Ok(false) => KeyringState::Ready,
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
}
