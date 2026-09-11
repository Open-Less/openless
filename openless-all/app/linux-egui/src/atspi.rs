//! AT-SPI fallback uses the accessibility bus and a unique bus owner/object
//! pair. Only the focused text object is read; passwords are never queried.
use crate::context::{platform, TargetSnapshot, CONTEXT_PROTOCOL_VERSION};
use dbus::blocking::{stdintf::org_freedesktop_dbus::Properties, Connection};
use openless_core::BackendError;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

const ACCESSIBLE: &str = "org.a11y.atspi.Accessible";
const TEXT: &str = "org.a11y.atspi.Text";
const ROOT: &str = "/org/a11y/atspi/accessible/root";
type Object = (String, dbus::Path<'static>);

fn connect() -> Result<Connection, BackendError> {
    let session = Connection::new_session().map_err(platform)?;
    let (address,): (String,) = session
        .with_proxy("org.a11y.Bus", "/org/a11y/bus", Duration::from_millis(500))
        .method_call("org.a11y.Bus", "GetAddress", ())
        .map_err(platform)?;
    let mut channel = dbus::channel::Channel::open_private(&address).map_err(platform)?;
    channel.register().map_err(platform)?;
    Ok(Connection::from(channel))
}

fn focused(connection: &Connection, object: &Object) -> bool {
    let states: Result<(Vec<u32>,), _> = connection
        .with_proxy(&object.0, object.1.clone(), Duration::from_millis(100))
        .method_call(ACCESSIBLE, "GetState", ());
    states.is_ok_and(|(s,)| {
        s.first()
            .is_some_and(|bits| bits & (1 << 12) != 0 && bits & (1 << 6) == 0)
    })
}

pub fn snapshot(
    expected: Option<&str>,
    include_text: bool,
) -> Result<TargetSnapshot, BackendError> {
    let connection = connect()?;
    let object = if let Some(expected) = expected {
        let (owner, path) = expected
            .strip_prefix("atspi:")
            .and_then(|s| s.split_once('|'))
            .ok_or_else(|| platform("invalid AT-SPI identity"))?;
        (
            owner.to_string(),
            dbus::Path::new(path.to_string()).map_err(platform)?,
        )
    } else {
        let root =
            connection.with_proxy("org.a11y.atspi.Registry", ROOT, Duration::from_millis(200));
        let (apps,): (Vec<Object>,) = root
            .method_call(ACCESSIBLE, "GetChildren", ())
            .map_err(platform)?;
        let mut queue: VecDeque<Object> = apps.into();
        let started = Instant::now();
        let mut visited = 0;
        let mut found = None;
        while let Some(candidate) = queue.pop_front() {
            if visited >= 512 || started.elapsed() > Duration::from_secs(2) {
                break;
            }
            visited += 1;
            if focused(&connection, &candidate) {
                found = Some(candidate);
                break;
            }
            let children: Result<(Vec<Object>,), _> = connection
                .with_proxy(
                    &candidate.0,
                    candidate.1.clone(),
                    Duration::from_millis(100),
                )
                .method_call(ACCESSIBLE, "GetChildren", ());
            if let Ok((children,)) = children {
                queue.extend(children.into_iter().take(128));
            }
        }
        found.ok_or_else(|| platform("no focused AT-SPI object"))?
    };
    if !focused(&connection, &object) {
        return Err(platform("AT-SPI target lost focus"));
    }
    let proxy = connection.with_proxy(&object.0, object.1.clone(), Duration::from_millis(300));
    let (role,): (String,) = proxy
        .method_call(ACCESSIBLE, "GetRoleName", ())
        .map_err(platform)?;
    let sensitive = role.to_ascii_lowercase().contains("password");
    let application = connection
        .with_proxy(&object.0, ROOT, Duration::from_millis(100))
        .get::<String>(ACCESSIBLE, "Name")
        .unwrap_or_default();
    let mut snapshot = TargetSnapshot {
        version: CONTEXT_PROTOCOL_VERSION,
        target: format!("atspi:{}|{}", object.0, object.1),
        application,
        sensitive,
        text: None,
        cursor: 0,
    };
    if include_text && !sensitive {
        let count: i32 = proxy.get(TEXT, "CharacterCount").map_err(platform)?;
        if !(0..=16384).contains(&count) {
            return Err(platform("AT-SPI document exceeds observation limit"));
        }
        let cursor: i32 = proxy.get(TEXT, "CaretOffset").map_err(platform)?;
        let (text,): (String,) = proxy
            .method_call(TEXT, "GetText", (0_i32, count))
            .map_err(platform)?;
        snapshot.text = Some(text);
        snapshot.cursor = cursor.max(0) as usize;
    }
    if !focused(&connection, &object) {
        return Err(platform("AT-SPI target changed during capture"));
    }
    crate::context::validate_snapshot(&snapshot, expected, include_text)?;
    Ok(snapshot)
}
