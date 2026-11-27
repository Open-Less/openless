//! New shared-UI system commands. No toolkit dependency: Qt and GTK use this same boundary.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::{json, Value};

use crate::{data_dir, find_recording, required_str, Host};

pub(crate) enum Picker<'a> {
    Directory { title: &'a str },
    OpenZip,
    Save { file_name: &'a str, filter: &'a str },
}

fn selection(status: i32, output: &[u8]) -> Result<Option<PathBuf>, String> {
    match status {
        0 => {
            let path =
                String::from_utf8(output.to_vec()).map_err(|_| "Invalid file picker response")?;
            let path = path.trim_end_matches(['\r', '\n']);
            if path.is_empty() {
                return Err("File picker returned an empty path".into());
            }
            let path = PathBuf::from(path);
            if !path.is_absolute() {
                return Err("File picker must return an absolute path".into());
            }
            Ok(Some(path))
        }
        1 => Ok(None),
        _ => Err(format!("File picker failed (exit {status})")),
    }
}

pub(crate) fn pick(picker: Picker<'_>) -> Result<Option<PathBuf>, String> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
    let (kde, gnome): (Vec<String>, Vec<String>) = match picker {
        Picker::Directory { title } => (
            vec![
                "--title".into(),
                title.into(),
                "--getexistingdirectory".into(),
                home.clone(),
            ],
            vec![
                "--file-selection".into(),
                "--directory".into(),
                format!("--title={title}"),
                format!("--filename={home}/"),
            ],
        ),
        Picker::OpenZip => (
            vec![
                "--getopenfilename".into(),
                home.clone(),
                "Style Pack ZIP (*.zip)".into(),
            ],
            vec![
                "--file-selection".into(),
                "--file-filter=Style Pack ZIP | *.zip".into(),
                format!("--filename={home}/"),
            ],
        ),
        Picker::Save { file_name, filter } => {
            let safe_name = file_name.replace(['/', '\\', '\n', '\r'], "_");
            let path = format!("{home}/{safe_name}");
            (
                vec!["--getsavefilename".into(), path.clone(), filter.into()],
                vec![
                    "--file-selection".into(),
                    "--save".into(),
                    "--confirm-overwrite".into(),
                    format!("--filename={path}"),
                ],
            )
        }
    };
    for (program, args) in [("kdialog", kde), ("zenity", gnome)] {
        match Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .output()
        {
            Ok(Output { status, stdout, .. }) => {
                return selection(status.code().unwrap_or(-1), &stdout)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("Cannot open file picker: {error}")),
        }
    }
    Err("No native file picker is installed (install kdialog or zenity)".into())
}

// Icons are optional presentation metadata. Resolve only local PNG assets, never
// execute .desktop Exec fields, return arbitrary file contents, or fetch icon URLs.
fn app_icons(names: impl IntoIterator<Item = String>) -> BTreeMap<String, String> {
    let requested: Vec<_> = names.into_iter().take(64).collect();
    if requested.is_empty() {
        return BTreeMap::new();
    }
    let mut dirs = Vec::new();
    if let Some(home) = std::env::var_os("XDG_DATA_HOME") {
        dirs.push(PathBuf::from(home));
    } else if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join(".local/share"));
    }
    dirs.extend(
        std::env::var_os("XDG_DATA_DIRS")
            .map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
            .unwrap_or_else(|| vec!["/usr/local/share".into(), "/usr/share".into()]),
    );
    let mut result = BTreeMap::new();
    for directory in &dirs {
        let Ok(entries) = std::fs::read_dir(directory.join("applications")) else {
            continue;
        };
        for entry in entries.flatten().take(4096) {
            let path = entry.path();
            if path.extension().and_then(|x| x.to_str()) != Some("desktop") {
                continue;
            }
            if std::fs::metadata(&path).is_ok_and(|m| m.len() > 128 * 1024) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let mut section = false;
            let (mut name, mut icon, mut class) = ("", "", "");
            for line in text.lines() {
                if line.starts_with('[') {
                    section = line == "[Desktop Entry]";
                }
                if !section {
                    continue;
                }
                if let Some(value) = line.strip_prefix("Name=") {
                    name = value;
                }
                if let Some(value) = line.strip_prefix("Icon=") {
                    icon = value;
                }
                if let Some(value) = line.strip_prefix("StartupWMClass=") {
                    class = value;
                }
            }
            let stem = path
                .file_stem()
                .and_then(|x| x.to_str())
                .unwrap_or_default();
            let matching: Vec<_> = requested
                .iter()
                .filter(|key| {
                    let id = key
                        .strip_prefix("linux:")
                        .unwrap_or(key)
                        .trim_end_matches(".desktop");
                    [name, class, stem].iter().any(|candidate| {
                        !candidate.is_empty() && candidate.eq_ignore_ascii_case(id)
                    })
                })
                .collect();
            if matching.is_empty() || icon.is_empty() {
                continue;
            }
            let mut paths = Vec::new();
            if Path::new(icon).is_absolute() {
                paths.push(PathBuf::from(icon));
            } else if !icon.contains(['/', '\\']) && !icon.starts_with('.') {
                for dir in &dirs {
                    for size in ["64x64", "48x48", "128x128", "32x32"] {
                        paths.push(dir.join(format!(
                            "icons/hicolor/{size}/apps/{}.png",
                            icon.trim_end_matches(".png")
                        )));
                    }
                    paths.push(dir.join(format!("pixmaps/{}.png", icon.trim_end_matches(".png"))));
                }
            }
            for file in paths {
                if !std::fs::metadata(&file).is_ok_and(|m| m.is_file() && m.len() <= 1024 * 1024) {
                    continue;
                }
                let Ok(bytes) = std::fs::read(file) else {
                    continue;
                };
                if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
                    continue;
                }
                let data = format!("data:image/png;base64,{}", crate::base64_encode(&bytes));
                for key in &matching {
                    result.entry((*key).clone()).or_insert_with(|| data.clone());
                }
                break;
            }
        }
    }
    result
}

pub(crate) fn dispatch(host: &Host, command: &str, args: &Value) -> Option<Result<Value, String>> {
    let known = matches!(
        command,
        "get_app_style_routing"
            | "refresh_app_style_catalog"
            | "set_app_style_assignment"
            | "clear_input_app_history"
            | "get_input_app_icons"
            | "get_activity_app_icons"
            | "pick_directory"
            | "open_external_url"
            | "export_audio_recording"
            | "trigger_microphone_prompt"
    );
    if !known {
        return None;
    }
    Some((|| {
        // Dialog-only and desktop-open operations do not require a running Core.
        match command {
            "pick_directory" => {
                return Ok(json!(pick(Picker::Directory {
                    title: args
                        .get("title")
                        .and_then(Value::as_str)
                        .unwrap_or("OpenLess"),
                })?))
            }
            "open_external_url" => {
                openless_linux_host::open_external(required_str(args, "url")?)
                    .map_err(|e| e.to_string())?;
                return Ok(Value::Null);
            }
            _ => {}
        }
        let backend = host.backend.as_deref().ok_or("Backend unavailable")?;
        match command {
            "get_app_style_routing" => Ok(crate::json_value(
                backend.app_style_routing().map_err(|e| e.to_string())?,
            )),
            "refresh_app_style_catalog" => {
                let runtime = host.runtime.as_ref().ok_or("Runtime unavailable")?;
                Ok(crate::json_value(
                    runtime
                        .block_on(backend.refresh_app_style_catalog())
                        .map_err(|e| e.to_string())?,
                ))
            }
            "set_app_style_assignment" => {
                let assignment = serde_json::from_value(
                    args.get("assignment")
                        .cloned()
                        .ok_or("Missing assignment")?,
                )
                .map_err(|e| e.to_string())?;
                Ok(crate::json_value(
                    backend
                        .set_app_style_assignment(assignment)
                        .map_err(|e| e.to_string())?,
                ))
            }
            "clear_input_app_history" => Ok(crate::json_value(
                backend
                    .clear_input_app_history()
                    .map_err(|e| e.to_string())?,
            )),
            "get_input_app_icons" => Ok(json!(app_icons(
                backend
                    .app_style_routing()
                    .map_err(|e| e.to_string())?
                    .apps
                    .into_iter()
                    .map(|entry| entry.app.id)
            ))),
            "get_activity_app_icons" => {
                let year = args
                    .get("year")
                    .and_then(Value::as_u64)
                    .and_then(|v| u32::try_from(v).ok())
                    .ok_or("Invalid year")?;
                Ok(json!(app_icons(
                    backend
                        .list_activity_apps(year)
                        .map_err(|e| e.to_string())?
                        .into_iter()
                        .take(5)
                        .map(|(name, _)| name)
                )))
            }
            "trigger_microphone_prompt" => {
                let runtime = host.runtime.as_ref().ok_or("Runtime unavailable")?;
                runtime
                    .block_on(backend.services().platform.request_microphone_permission())
                    .map_err(|e| e.to_string())?;
                Ok(Value::Null)
            }
            "export_audio_recording" => {
                let id = required_str(args, "sessionId")?;
                uuid::Uuid::parse_str(id).map_err(|_| "Invalid recording session ID")?;
                let entry = backend
                    .list_history()
                    .map_err(|e| e.to_string())?
                    .into_iter()
                    .find(|entry| entry.id == id)
                    .ok_or("History entry not found")?;
                let source =
                    find_recording(&data_dir(), id).ok_or("Recording archive not found")?;
                let file_name = format!("openless-recording-{id}.wav");
                let configured = backend.get_preferences().quick_note_export_directory;
                let destination = if entry.source == openless_core::HistorySource::QuickNote
                    && !configured.trim().is_empty()
                {
                    let directory = PathBuf::from(configured.trim());
                    if !directory.is_absolute() {
                        return Err("Export directory must be absolute".into());
                    }
                    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
                    directory.join(file_name)
                } else {
                    pick(Picker::Save {
                        file_name: &file_name,
                        filter: "WAV audio (*.wav)",
                    })?
                    .ok_or("user cancelled")?
                };
                let bytes = std::fs::read(source).map_err(|e| e.to_string())?;
                let saved = openless_linux_host::atomic_save(&destination, &bytes)
                    .map_err(|e| e.to_string())?;
                Ok(json!(saved))
            }
            _ => unreachable!("known command must have an implementation"),
        }
    })())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn new_commands_use_real_core_state_and_validate_system_inputs() {
        use std::sync::{Arc, Mutex};
        let directory =
            std::env::temp_dir().join(format!("openless-system-api-{}", uuid::Uuid::new_v4()));
        let backend = Arc::new(
            openless_core::OpenLessBackend::new(
                openless_core::BackendConfig {
                    data_dir: directory.clone(),
                    ..Default::default()
                },
                openless_core::BackendDependencies::unsupported(),
            )
            .unwrap(),
        );
        let (events, _) = tokio::sync::broadcast::channel(16);
        let host = Host {
            backend: Some(backend),
            runtime: None,
            settings_runtime: None,
            qa_expanded: Mutex::new(false),
            cloud_sync_status_cache: Mutex::new(None),
            capsule_transcript_visible: Mutex::new(false),
            capsule_language_menu: Mutex::new(false),
            shortcut_recording_active: Mutex::new(false),
            host_events: events,
            selection_preview_pending: std::sync::atomic::AtomicBool::new(false),
            voice_edit: Arc::new(crate::voice_edit::VoiceEdit::default()),
            mic_monitor: Arc::new(Mutex::new(None)),
            host_actions: None,
            action_wake_rx: Mutex::new(None),
        };
        let state = dispatch(&host, "get_app_style_routing", &json!({}))
            .unwrap()
            .unwrap();
        assert!(state["apps"].is_array());
        assert!(dispatch(&host, "set_app_style_assignment", &json!({}))
            .unwrap()
            .is_err());
        assert!(dispatch(&host, "clear_input_app_history", &json!({}))
            .unwrap()
            .unwrap()["apps"]
            .is_array());
        assert_eq!(
            dispatch(&host, "get_input_app_icons", &json!({}))
                .unwrap()
                .unwrap(),
            json!({})
        );
        assert!(dispatch(
            &host,
            "export_audio_recording",
            &json!({"sessionId":"../../secret"})
        )
        .unwrap()
        .is_err());
        assert!(dispatch(
            &host,
            "open_external_url",
            &json!({"url":"file:///etc/passwd"})
        )
        .unwrap()
        .is_err());
        assert!(dispatch(&host, "unrecognized_command", &json!({})).is_none());
        drop(host);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn picker_cancel_is_not_confused_with_missing_or_failed_tool() {
        assert!(selection(1, b"").unwrap().is_none());
        assert!(selection(2, b"").is_err());
        assert!(selection(0, b"").is_err());
        assert!(selection(0, b"relative.zip\n").is_err());
        assert_eq!(
            selection(0, b"/tmp/selected file.zip\n").unwrap().unwrap(),
            Path::new("/tmp/selected file.zip")
        );
    }
}
