//! Public Ice dictionary refreshes. No editor text or user dictionaries enter this module.
//! A ready snapshot is selected only on the next engine initialization.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const BUNDLED_DIRECTORY: &str = "rime-shared-1.17.0-ice-da1fbe60-v1";
const BUNDLED_REVISION: &str = "da1fbe602e38f26db846fa10120ee64c2b0324c0";
const TABLES: [&str; 5] = ["8105", "base", "ext", "tencent", "others"];
const DAY: u64 = 86400;
static RUNNING: AtomicBool = AtomicBool::new(false);
static META: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct State {
    ready: String,
    active: String,
    previous: String,
    checked: u64,
    attempted: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DictionaryStatus {
    pub data_path: PathBuf,
    pub revision: String,
    pub pending: bool,
    pub checked: u64,
}

fn root(base: &Path) -> PathBuf {
    base.join("public-updates-v1")
}
fn revision(value: &str) -> &str {
    if value.is_empty() {
        BUNDLED_REVISION
    } else {
        value
    }
}
fn valid_revision(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
fn read_state(base: &Path) -> State {
    fs::read(root(base).join("state.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}
fn save_state(base: &Path, state: &State) -> Result<(), String> {
    fs::create_dir_all(root(base)).map_err(|_| "create dictionary cache")?;
    crate::persistence::atomic_write(
        &root(base).join("state.json"),
        &serde_json::to_vec(state).map_err(|_| "encode dictionary metadata")?,
    )
    .map_err(|_| "save dictionary metadata".into())
}
fn snapshot(base: &Path, sha: &str) -> Option<PathBuf> {
    if sha.is_empty() || sha == BUNDLED_REVISION {
        return Some(base.to_owned());
    }
    if !valid_revision(sha) {
        return None;
    }
    let path = root(base).join(sha);
    if fs::symlink_metadata(&path).ok()?.file_type().is_symlink() {
        return None;
    }
    if fs::read_to_string(path.join("openless-public-revision.txt"))
        .ok()?
        .trim()
        != sha
    {
        return None;
    }
    for name in TABLES
        .map(|t| format!("ice_{t}.dict.yaml"))
        .into_iter()
        .chain([
            "openless_pinyin_private.schema.yaml".into(),
            "openless_pinyin_learning.schema.yaml".into(),
            "rime_ice.dict.yaml".into(),
        ])
    {
        let meta = fs::symlink_metadata(path.join(name)).ok()?;
        if !meta.is_file() || meta.len() == 0 {
            return None;
        }
    }
    Some(path)
}
fn status(base: &Path, state: &State) -> DictionaryStatus {
    DictionaryStatus {
        data_path: snapshot(base, &state.ready).unwrap_or_else(|| base.to_owned()),
        revision: revision(&state.ready).to_owned(),
        pending: revision(&state.ready) != revision(&state.active),
        checked: state.checked,
    }
}

/// Purely local, fast startup selection. Invalid/incomplete snapshots fall back to bundled data.
pub fn select(base: &Path) -> DictionaryStatus {
    let _guard = META.lock();
    let mut state = read_state(base);
    let repair = snapshot(base, &state.ready).is_none();
    if repair {
        state.ready.clear();
    }
    if repair || revision(&state.active) != revision(&state.ready) {
        state.active = state.ready.clone();
        let _ = save_state(base, &state);
    }
    status(base, &state)
}

/// Called if Rime rejects a staged public snapshot; never touches rime-user or its databases.
pub fn reject(base: &Path) -> DictionaryStatus {
    let _guard = META.lock();
    let mut state = read_state(base);
    state.ready = if snapshot(base, &state.previous).is_some() {
        state.previous.clone()
    } else {
        String::new()
    };
    state.active = state.ready.clone();
    state.previous.clear();
    state.checked = 0;
    let _ = save_state(base, &state);
    status(base, &state)
}

fn due(state: &State, now: u64) -> bool {
    let elapsed = |stamp: u64, interval: u64| {
        stamp == 0 || now.saturating_sub(stamp) >= interval || stamp > now.saturating_add(DAY)
    };
    revision(&state.ready) == revision(&state.active)
        && elapsed(state.checked, 7 * DAY)
        && elapsed(state.attempted, DAY)
}

async fn download(client: &reqwest::Client, url: &str, limit: usize) -> Result<Vec<u8>, String> {
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|_| "public dictionary network unavailable")?;
    if !response.status().is_success() {
        return Err("public dictionary server unavailable".into());
    }
    if response.content_length().is_some_and(|n| n > limit as u64) {
        return Err("public dictionary response too large".into());
    }
    let mut data = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "public dictionary download interrupted")?
    {
        if data.len().saturating_add(chunk.len()) > limit {
            return Err("public dictionary response too large".into());
        }
        data.extend_from_slice(&chunk);
    }
    Ok(data)
}

fn table(source: &[u8], name: &str, sha: &str) -> Result<Vec<u8>, String> {
    let text = std::str::from_utf8(source).map_err(|_| "invalid dictionary encoding")?;
    if text.contains('\0') {
        return Err("invalid dictionary data".into());
    }
    let (header, body) = text
        .split_once("\n...\n")
        .ok_or("missing dictionary header")?;
    // The base table has a substantial upstream change log before its YAML header.
    if header.len() > 1024 * 1024 || header.contains("import_tables:") {
        return Err("unsupported dictionary header".into());
    }
    let expected = [format!("name: {name}"), format!("name: cn_dicts/{name}")];
    let line = header
        .lines()
        .find(|line| expected.iter().any(|n| line.trim() == n))
        .ok_or("unexpected dictionary name")?;
    if !body
        .lines()
        .any(|line| !line.starts_with('#') && line.contains('\t'))
    {
        return Err("empty dictionary".into());
    }
    let header = header.replacen(line, &format!("name: ice_{name}"), 1);
    Ok(format!("# OpenLess: flattened public table name; upstream iDvel/rime-ice {sha}.\n{header}\n...\n{body}").into_bytes())
}

fn copy_base(base: &Path, stage: &Path) -> Result<(), String> {
    fs::create_dir(stage).map_err(|_| "create dictionary snapshot")?;
    for entry in fs::read_dir(base).map_err(|_| "read bundled dictionaries")? {
        let entry = entry.map_err(|_| "read dictionary resource")?;
        if entry
            .file_type()
            .map_err(|_| "read dictionary resource type")?
            .is_file()
        {
            fs::copy(entry.path(), stage.join(entry.file_name()))
                .map_err(|_| "copy bundled dictionary resource")?;
        }
    }
    Ok(())
}

/// At most weekly on success and daily after failures. This only stages public data;
/// it never reloads a live composition or writes learned words to the network.
pub async fn update(base: &Path, force: bool) -> Result<DictionaryStatus, String> {
    if RUNNING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
        .is_err()
    {
        return Ok(status(base, &read_state(base)));
    }
    struct Running;
    impl Drop for Running {
        fn drop(&mut self) {
            RUNNING.store(false, Ordering::Release);
        }
    }
    let _running = Running;
    if fs::read_to_string(base.join("openless-data-version.txt"))
        .map_err(|_| "bundled dictionaries not ready")?
        .trim()
        != "ice-da1fbe60-v1"
    {
        return Err("unsupported bundled dictionary version".into());
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    {
        let _guard = META.lock();
        let mut state = read_state(base);
        if (!force && !due(&state, now)) || revision(&state.ready) != revision(&state.active) {
            return Ok(status(base, &state));
        }
        state.attempted = now;
        save_state(base, &state)?;
    }
    let client = reqwest::Client::builder()
        .user_agent("OpenLess-public-dictionary/1")
        .timeout(Duration::from_secs(180))
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "create dictionary download client")?;
    let head: serde_json::Value = serde_json::from_slice(
        &download(
            &client,
            "https://api.github.com/repos/iDvel/rime-ice/commits/main",
            1024 * 1024,
        )
        .await?,
    )
    .map_err(|_| "invalid public revision metadata")?;
    let sha = head["sha"]
        .as_str()
        .filter(|sha| valid_revision(sha))
        .ok_or("invalid public revision")?;
    if sha == revision(&read_state(base).ready) {
        let _guard = META.lock();
        let mut state = read_state(base);
        state.checked = now;
        save_state(base, &state)?;
        return Ok(status(base, &state));
    }
    let stage = root(base).join(format!(".stage-{}", uuid::Uuid::new_v4()));
    let result = async {
        copy_base(base, &stage)?;
        for name in TABLES {
            let bytes = download(&client, &format!("https://raw.githubusercontent.com/iDvel/rime-ice/{sha}/cn_dicts/{name}.dict.yaml"), 40 * 1024 * 1024).await?;
            fs::write(stage.join(format!("ice_{name}.dict.yaml")), table(&bytes, name, sha)?).map_err(|_| "write public dictionary table")?;
        }
        let license = download(&client, &format!("https://raw.githubusercontent.com/iDvel/rime-ice/{sha}/LICENSE"), 65536).await?;
        if license.is_empty() { return Err("missing public dictionary license".into()); }
        fs::write(stage.join("rime-ice-LICENSE"), license).map_err(|_| "write dictionary license")?;
        fs::write(stage.join("openless-public-revision.txt"), sha).map_err(|_| "write dictionary revision")?;
        let destination = root(base).join(sha);
        if destination.exists() {
            // Never overwrite a snapshot an existing engine might still read.
            if snapshot(base, sha).is_none() { return Err("dictionary snapshot already exists but is incomplete".into()); }
        } else { fs::rename(&stage, &destination).map_err(|_| "publish dictionary snapshot")?; }
        let _guard = META.lock();
        let mut state = read_state(base);
        state.previous = state.ready.clone(); state.ready = sha.to_owned(); state.checked = now;
        save_state(base, &state)?;
        let result = status(base, &state);
        let keep = [revision(&state.active).to_owned(), revision(&state.ready).to_owned(), revision(&state.previous).to_owned()];
        drop(_guard);
        // Retain the running, pending and rollback versions only. This root
        // contains public snapshots, never the adjacent personal profile.
        if let Ok(entries) = fs::read_dir(root(base)) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if valid_revision(&name) && !keep.contains(&name) && snapshot(base, &name).is_some() {
                    let _ = fs::remove_dir_all(entry.path());
                }
            }
        }
        Ok(result)
    }.await;
    // Only our unique temporary cache is removed. Bundled data and personal databases are untouched.
    let _ = fs::remove_dir_all(&stage);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    #[ignore = "explicit public upstream download; never reads or sends personal dictionaries"]
    async fn live_public_snapshot() {
        let source = PathBuf::from(
            std::env::var_os("OPENLESS_IME_TEST_RESOURCES")
                .expect("bundled public resources required"),
        );
        let retained = std::env::var_os("OPENLESS_IME_TEST_OUTPUT");
        let temp = retained.clone().map(PathBuf::from).unwrap_or_else(|| {
            std::env::temp_dir().join(format!("openless-public-live-{}", uuid::Uuid::new_v4()))
        });
        fs::create_dir_all(&temp).unwrap();
        let base = temp.join(BUNDLED_DIRECTORY);
        copy_base(&source, &base).unwrap();
        // Represent an older installed release so the full download/stage path
        // is exercised even when upstream currently matches the bundled revision.
        save_state(
            &base,
            &State {
                ready: "0".repeat(40),
                active: "0".repeat(40),
                ..Default::default()
            },
        )
        .unwrap();
        let result = update(&base, true).await.unwrap();
        let staged = root(&base).join(&result.revision);
        assert!(result.pending);
        assert!(staged.join("openless-public-revision.txt").is_file());
        for name in TABLES {
            assert!(
                fs::metadata(staged.join(format!("ice_{name}.dict.yaml")))
                    .unwrap()
                    .len()
                    > 1000
            );
        }
        assert!(select(&base).data_path.is_dir());
        println!("Public snapshot validated: {}", result.revision);
        if retained.is_none() {
            fs::remove_dir_all(temp).unwrap();
        }
    }
    #[test]
    fn public_table_validation_and_flattening() {
        let source = b"# upstream\n---\nname: base\nversion: x\n...\nword\tword\t1\n";
        let output = String::from_utf8(table(source, "base", BUNDLED_REVISION).unwrap()).unwrap();
        assert!(output.contains("name: ice_base"));
        assert!(table(source, "others", BUNDLED_REVISION).is_err());
        assert!(table(b"name: base\n...\n", "base", BUNDLED_REVISION).is_err());
        assert!(!valid_revision("../../profile"));
    }
    #[test]
    fn refresh_schedule_and_pending_snapshot() {
        let mut state = State::default();
        assert!(due(&state, 10 * DAY));
        state.attempted = 10 * DAY;
        assert!(!due(&state, 10 * DAY + 10));
        assert!(due(&state, 11 * DAY));
        state.checked = 10 * DAY;
        assert!(!due(&state, 16 * DAY));
        assert!(due(&state, 17 * DAY));
        state.ready = "a".repeat(40);
        assert!(!due(&state, 30 * DAY));
    }
    #[test]
    fn snapshot_selection_and_rollback_preserve_personal_data() {
        let temp =
            std::env::temp_dir().join(format!("openless-ime-update-{}", uuid::Uuid::new_v4()));
        let base = temp.join(BUNDLED_DIRECTORY);
        fs::create_dir_all(&base).unwrap();
        let personal = temp.join("rime-user");
        fs::create_dir(&personal).unwrap();
        fs::write(personal.join("pinyin_simp.userdb"), b"personal sentinel").unwrap();
        assert_eq!(select(&base).data_path, base);
        let sha = "a".repeat(40);
        let dir = root(&base).join(&sha);
        fs::create_dir_all(&dir).unwrap();
        for name in TABLES
            .map(|t| format!("ice_{t}.dict.yaml"))
            .into_iter()
            .chain([
                "openless_pinyin_private.schema.yaml".into(),
                "openless_pinyin_learning.schema.yaml".into(),
                "rime_ice.dict.yaml".into(),
            ])
        {
            fs::write(dir.join(name), b"fixture").unwrap();
        }
        fs::write(dir.join("openless-public-revision.txt"), &sha).unwrap();
        save_state(
            &base,
            &State {
                ready: sha,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(select(&base).data_path, dir);
        assert!(!select(&base).pending);
        assert_eq!(reject(&base).data_path, base);
        save_state(
            &base,
            &State {
                ready: "../../rime-user".into(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(select(&base).data_path, base);
        assert!(
            read_state(&base).ready.is_empty(),
            "bad metadata must not prevent future updates"
        );
        assert_eq!(
            fs::read(personal.join("pinyin_simp.userdb")).unwrap(),
            b"personal sentinel"
        );
        fs::remove_dir_all(temp).unwrap();
    }
}
