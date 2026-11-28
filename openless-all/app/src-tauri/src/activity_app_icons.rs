//! Local desktop app icons. The webview can ask for ranked apps, not arbitrary paths.

use base64::{engine::general_purpose::STANDARD, Engine};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::io::{Read, Write};
use std::path::PathBuf;

const MAX_PNG_BYTES: usize = 128 * 1024;
const PNG_HEADER: &[u8] = b"\x89PNG\r\n\x1a\n";

fn cache_path(name: &str) -> Option<PathBuf> {
    let key = Sha256::digest(name.as_bytes());
    Some(
        crate::persistence::data_dir()
            .ok()?
            .join("activity-app-icons")
            .join(format!("{key:x}.png")),
    )
}

fn read_cached(name: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(cache_path(name)?)
        .ok()?
        .take(MAX_PNG_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() <= MAX_PNG_BYTES && bytes.starts_with(PNG_HEADER)).then_some(bytes)
}

fn cache(name: &str, bytes: &[u8]) -> Option<()> {
    let path = cache_path(name)?;
    let directory = path.parent()?;
    std::fs::create_dir_all(directory).ok()?;
    let mut tmp = tempfile::NamedTempFile::new_in(directory).ok()?;
    tmp.write_all(bytes).ok()?;
    tmp.persist(path).ok()?;
    Some(())
}

unsafe extern "C" fn receive_png(output: *mut c_void, bytes: *const u8, len: usize) {
    if !bytes.is_null() && (PNG_HEADER.len()..=MAX_PNG_BYTES).contains(&len) {
        // The native callback is synchronous, and the caller owns the output Vec.
        let bytes = unsafe { std::slice::from_raw_parts(bytes, len) };
        if bytes.starts_with(PNG_HEADER) {
            unsafe { &mut *output.cast::<Vec<u8>>() }.extend_from_slice(bytes);
        }
    }
}

fn data_urls(rows: Vec<(String, Vec<u8>)>) -> BTreeMap<String, String> {
    rows.into_iter()
        .map(|(name, png)| {
            (
                name,
                format!("data:image/png;base64,{}", STANDARD.encode(png)),
            )
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn macos_png(name: &str, bundle_id: Option<&str>) -> Option<Vec<u8>> {
    use std::ffi::{c_char, CString};
    unsafe extern "C" {
        fn openless_macos_activity_app_icon(
            name: *const c_char,
            bundle_id: *const c_char,
            output: *mut c_void,
            receive: unsafe extern "C" fn(*mut c_void, *const u8, usize),
        );
    }
    let name = CString::new(name).ok()?;
    let bundle_id = bundle_id.and_then(|id| CString::new(id).ok());
    let mut png = Vec::<u8>::new();
    unsafe {
        openless_macos_activity_app_icon(
            name.as_ptr(),
            bundle_id
                .as_ref()
                .map_or(std::ptr::null(), |id| id.as_ptr()),
            (&mut png as *mut Vec<u8>).cast(),
            receive_png,
        );
    }
    (!png.is_empty()).then_some(png)
}

#[cfg(target_os = "macos")]
#[derive(Debug, PartialEq, Eq)]
struct MacIconRequest {
    key: String,
    name: String,
    bundle: Option<String>,
}

#[cfg(target_os = "macos")]
impl MacIconRequest {
    fn cache_key(&self) -> String {
        match &self.bundle {
            Some(id) => format!("mac:bundle:{id}"),
            None => format!("mac:name:{}", self.name),
        }
    }
}

#[cfg(target_os = "macos")]
fn macos_requests(
    names: Vec<String>,
    recorded_apps: impl IntoIterator<Item = (String, String)>,
) -> Vec<MacIconRequest> {
    let mut bundles = BTreeMap::new();
    for (name, id) in recorded_apps {
        if names.contains(&name) && !id.is_empty() {
            let bundle = bundles.entry(name).or_insert_with(|| Some(id.clone()));
            if bundle.as_deref() != Some(id.as_str()) {
                *bundle = None;
            }
        }
    }
    names
        .into_iter()
        .filter_map(|name| {
            let bundle = match bundles.remove(&name) {
                // Ambiguous names must not use a cached icon or a running-app guess.
                Some(None) => return None,
                found => found.flatten(),
            };
            Some(MacIconRequest { key: name.clone(), name, bundle })
        })
        .collect()
}

#[cfg(target_os = "macos")]
pub(crate) async fn macos_icons(
    app: &tauri::AppHandle,
    names: Vec<String>,
    recorded_apps: impl IntoIterator<Item = (String, String)>,
) -> BTreeMap<String, String> {
    macos_icons_for_requests(app, macos_requests(names, recorded_apps)).await
}

#[cfg(target_os = "macos")]
pub(crate) async fn macos_input_app_icons(
    app: &tauri::AppHandle,
    inputs: Vec<openless_core::app_style_routing::InputApp>,
) -> BTreeMap<String, String> {
    let requests = inputs.into_iter().filter_map(|input| {
        let bundle = input.macos_bundle_id()?.to_owned();
        Some(MacIconRequest { key: input.id, name: input.name, bundle: Some(bundle) })
    }).collect();
    macos_icons_for_requests(app, requests).await
}

#[cfg(target_os = "macos")]
async fn macos_icons_for_requests(
    app: &tauri::AppHandle,
    requests: Vec<MacIconRequest>,
) -> BTreeMap<String, String> {
    let cached = tauri::async_runtime::spawn_blocking(move || {
        let mut found = Vec::new();
        let mut missing = Vec::new();
        for request in requests {
            if let Some(png) = read_cached(&request.cache_key()) {
                found.push((request.key, png));
            } else {
                missing.push(request);
            }
        }
        (found, missing)
    })
    .await;
    let Ok((mut found, missing)) = cached else {
        return BTreeMap::new();
    };
    if !missing.is_empty() {
        let (send, receive) = tokio::sync::oneshot::channel();
        // AppKit drawing remains on its owning thread. Disk I/O never runs here.
        if app
            .run_on_main_thread(move || {
                let rows: Vec<_> = missing
                    .into_iter()
                    .filter_map(|request| {
                        macos_png(&request.name, request.bundle.as_deref())
                            .map(|png| (request, png))
                    })
                    .collect();
                let _ = send.send(rows);
            })
            .is_ok()
        {
            if let Ok(rows) = receive.await {
                if let Ok(mut rows) = tauri::async_runtime::spawn_blocking(move || {
                    rows.into_iter()
                        .map(|(request, png)| {
                            let _ = cache(&request.cache_key(), &png);
                            (request.key, png)
                        })
                        .collect()
                })
                .await
                {
                    found.append(&mut rows);
                }
            }
        }
    }
    data_urls(found)
}

#[cfg(all(test, target_os = "macos"))]
mod macos_tests {
    use super::*;

    #[test]
    fn a_bundle_is_the_cache_identity_and_an_ambiguous_name_is_not_resolved() {
        let requests = macos_requests(
            vec!["Browser".into(), "Editor".into(), "Legacy".into()],
            [
                ("Browser".into(), "com.example.browser".into()),
                ("Editor".into(), "com.example.editor1".into()),
                ("Editor".into(), "com.example.editor2".into()),
                ("Editor".into(), "com.example.editor1".into()),
            ],
        );
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].cache_key(), "mac:bundle:com.example.browser");
        assert_eq!(requests[1].cache_key(), "mac:name:Legacy");
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use super::*;
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::path::Path;
    use std::sync::{Mutex, OnceLock};

    unsafe extern "C" {
        fn openless_windows_activity_app_path(pid: u32, path: *mut u16, capacity: u32) -> u32;
        fn openless_windows_activity_app_icon(
            path: *const u16,
            output: *mut c_void,
            receive: unsafe extern "C" fn(*mut c_void, *const u8, usize),
        );
    }

    fn sources() -> &'static Mutex<BTreeMap<String, PathBuf>> {
        static SOURCES: OnceLock<Mutex<BTreeMap<String, PathBuf>>> = OnceLock::new();
        SOURCES.get_or_init(Mutex::default)
    }

    fn process_path(pid: u32) -> Option<PathBuf> {
        if pid == 0 {
            return None;
        }
        let mut utf16 = vec![0u16; 32768];
        let len = unsafe {
            openless_windows_activity_app_path(pid, utf16.as_mut_ptr(), utf16.len() as u32)
        };
        (len > 0 && (len as usize) < utf16.len())
            .then(|| PathBuf::from(OsString::from_wide(&utf16[..len as usize])))
    }

    fn png_for(name: &str, path: &Path) -> Option<Vec<u8>> {
        let path: Vec<_> = path.as_os_str().encode_wide().chain([0]).collect();
        let mut png = Vec::<u8>::new();
        unsafe {
            openless_windows_activity_app_icon(
                path.as_ptr(),
                (&mut png as *mut Vec<u8>).cast(),
                receive_png,
            );
        }
        if png.is_empty() {
            return None;
        }
        let _ = cache(name, &png);
        Some(png)
    }

    pub(crate) fn app_for_pid(pid: u32) -> Option<openless_core::app_style_routing::InputApp> {
        let path = process_path(pid)?;
        // The executable stem is for display and classification. Assignments use
        // a distinct key for each executable without persisting the path itself.
        let name = path.file_stem()?.to_str()?.to_string();
        let id = format!("windows:{:x}", Sha256::digest(path.to_string_lossy().to_lowercase().as_bytes()));
        if let Ok(mut sources) = sources().lock() {
            if !sources.contains_key(&id) && sources.len() < 512 {
                sources.entry(name.clone()).or_insert_with(|| path.clone());
                sources.insert(id.clone(), path.clone());
                let name = name.clone();
                let id = id.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    if read_cached(&id).is_none() {
                        if let Some(png) = png_for(&id, &path) {
                            if read_cached(&name).is_none() {
                                let _ = cache(&name, &png);
                            }
                        }
                    }
                });
            }
        }
        Some(openless_core::app_style_routing::InputApp { id, name })
    }

    pub(crate) async fn icons(names: Vec<String>) -> BTreeMap<String, String> {
        tauri::async_runtime::spawn_blocking(move || {
            let rows = names
                .into_iter()
                .filter_map(|name| {
                    let png = read_cached(&name).or_else(|| {
                        let path = sources().lock().ok()?.get(&name)?.clone();
                        png_for(&name, &path)
                    })?;
                    Some((name, png))
                })
                .collect();
            data_urls(rows)
        })
        .await
        .unwrap_or_default()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn current_process_resolves_to_an_executable_not_a_window_title() {
            let path = process_path(std::process::id()).unwrap();
            assert!(path.is_absolute());
            assert!(path.extension().unwrap().eq_ignore_ascii_case("exe"));
            assert!(process_path(0).is_none());
        }
    }
}

#[cfg(target_os = "windows")]
pub(crate) use windows::{app_for_pid, icons as windows_icons};
