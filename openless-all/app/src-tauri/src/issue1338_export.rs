//! Single-file diagnostic delivery. Never opens the live session in unit tests.
use super::{sink, LOG_NAME};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

type ExportResult<T> = Result<T, &'static str>;
#[derive(Debug)]
pub(super) struct Snapshot {
    pub(super) end: u64,
    pub(super) checkpoint: Vec<u8>,
    pub(super) incomplete: bool,
}

fn session_path() -> ExportResult<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty())
        .ok_or("issue1338_session_unavailable: capture unknown")?;
    Ok(PathBuf::from(base).join("OpenLess").join("diagnostics").join(LOG_NAME))
}

pub(super) fn export_log(source: &Path, target: &Path) -> ExportResult<()> {
    // The file's existence is not evidence that this run ever had a working sink.
    // Only an acknowledgment from the current writer can authorize a snapshot.
    export_with(source, &session_path()?, target, sink::snapshot)
}

fn canonical_target(target: &Path) -> ExportResult<PathBuf> {
    if target.exists() {
        return target.canonicalize().map_err(|_| "issue1338_target_unavailable: capture unknown");
    }
    let parent = target.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let name = target.file_name().ok_or("issue1338_target_unavailable: capture unknown")?;
    Ok(parent.canonicalize().map_err(|_| "issue1338_target_unavailable: capture unknown")?.join(name))
}

fn same_path(left: &Path, right: &Path) -> ExportResult<bool> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        // Canonical paths still need Windows ordinal case-insensitive comparison.
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn CompareStringOrdinal(a: *const u16, a_len: i32, b: *const u16, b_len: i32, ignore_case: i32) -> i32;
        }
        let a: Vec<u16> = left.as_os_str().encode_wide().collect();
        let b: Vec<u16> = right.as_os_str().encode_wide().collect();
        let a_len = i32::try_from(a.len()).map_err(|_| "issue1338_target_unavailable: capture unknown")?;
        let b_len = i32::try_from(b.len()).map_err(|_| "issue1338_target_unavailable: capture unknown")?;
        // SAFETY: both buffers remain alive and the lengths are checked above.
        let comparison = unsafe { CompareStringOrdinal(a.as_ptr(), a_len, b.as_ptr(), b_len, 1) };
        if comparison == 0 { return Err("issue1338_target_unavailable: capture unknown"); }
        Ok(comparison == 2) // CSTR_EQUAL
    }
    #[cfg(not(target_os = "windows"))]
    { Ok(left == right) }
}

fn export_with(source: &Path, session: &Path, target: &Path,
               capture: impl FnOnce() -> ExportResult<Snapshot>) -> ExportResult<()> {
    let source_id = source.canonicalize().map_err(|_| "issue1338_application_log_unavailable: capture unknown")?;
    let session_id = session.canonicalize().map_err(|_| "issue1338_session_unavailable: capture unknown")?;
    let target_id = canonical_target(target)?;
    // Resolve symlinks/junctions before comparison. No source is ever truncated,
    // even if a different target name is a hardlink to it: persist replaces its entry.
    if same_path(&target_id, &source_id)? || same_path(&target_id, &session_id)? {
        return Err("issue1338_source_target_alias: capture unknown");
    }
    let snapshot = capture()?;
    if snapshot.incomplete { return Err("issue1338_capture_incomplete: capture unknown"); }
    let mut diagnostics = File::open(session).map_err(|_| "issue1338_session_unavailable: capture unknown")?;
    let checkpoint_len = snapshot.checkpoint.len() as u64;
    let checkpoint_start = snapshot.end.checked_sub(checkpoint_len)
        .ok_or("issue1338_snapshot_invalid: capture unknown")?;
    // Validate the exact newly acknowledged checkpoint, not just a readable old
    // file or a length. Later appends are deliberately excluded, including half-lines.
    diagnostics.seek(SeekFrom::Start(checkpoint_start))
        .map_err(|_| "issue1338_snapshot_invalid: capture unknown")?;
    let mut actual = vec![0; snapshot.checkpoint.len()];
    diagnostics.read_exact(&mut actual).map_err(|_| "issue1338_snapshot_invalid: capture unknown")?;
    if actual != snapshot.checkpoint { return Err("issue1338_snapshot_invalid: capture unknown"); }
    diagnostics.rewind().map_err(|_| "issue1338_snapshot_invalid: capture unknown")?;
    let mut application = File::open(source).map_err(|_| "issue1338_application_log_unavailable: capture unknown")?;
    // Bound the ordinary log too; it may continue growing during export. Its
    // ordinary logger has no flush handshake, which is disclosed in the bundle.
    let application_end = application.metadata().map_err(|_| "issue1338_application_log_unavailable: capture unknown")?.len();
    let parent = target.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let mut staging = tempfile::NamedTempFile::new_in(parent)
        .map_err(|_| "issue1338_staging_failed: capture unknown")?;
    let result = (|| -> std::io::Result<()> {
        let copied = std::io::copy(&mut (&mut application).take(application_end), &mut staging)?;
        if copied != application_end { return Err(std::io::Error::other("short application snapshot")); }
        staging.write_all(b"\n--- OpenLess issue1338 diagnostic export v1 ---\napplication_log_tail_capture=unknown\ndiagnostic_bytes=end_at_current_writer_checkpoint\nhistorical_runs=not_current_coverage_unknown\n")?;
        let copied = std::io::copy(&mut diagnostics.take(snapshot.end), &mut staging)?;
        if copied != snapshot.end { return Err(std::io::Error::other("short diagnostic snapshot")); }
        staging.write_all(b"--- end issue1338 diagnostic export ---\n")?;
        staging.flush()?;
        Ok(())
    })();
    result.map_err(|_| "issue1338_staging_failed: capture unknown")?;
    // Never copy/truncate into an existing target. A failed persist leaves the
    // old target and both inputs intact; temp cleanup is owned by NamedTempFile.
    staging.persist(target).map_err(|_| "issue1338_publish_failed: capture unknown")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot() -> Snapshot {
        let checkpoint = b"{\"run_id\":\"current\"}\n".to_vec();
        Snapshot { end: b"old\n".len() as u64 + checkpoint.len() as u64, checkpoint, incomplete: false }
    }
    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("openless.log");
        let diag = dir.path().join("session.ndjson");
        let target = dir.path().join("export.log");
        std::fs::write(&app, b"ordinary\n").unwrap();
        let mut bytes = b"old\n".to_vec();
        bytes.extend(snapshot().checkpoint);
        std::fs::write(&diag, bytes).unwrap();
        std::fs::write(&target, b"preserve").unwrap();
        (dir, app, diag, target)
    }
    #[test]
    fn single_file_includes_ordinary_history_current_checkpoint_but_no_concurrent_tail() {
        let (_dir, app, diag, target) = fixture();
        let mut file = std::fs::OpenOptions::new().append(true).open(&diag).unwrap();
        file.write_all(b"{half-line").unwrap();
        export_with(&app, &diag, &target, || Ok(snapshot())).unwrap();
        let out = std::fs::read_to_string(target).unwrap();
        assert!(out.starts_with("ordinary\n"));
        assert!(out.contains("old\n{\"run_id\":\"current\"}\n"));
        assert!(out.contains("application_log_tail_capture=unknown"));
        assert!(!out.contains("half-line"));
    }
    #[test]
    fn stale_file_missing_file_loss_and_failed_capture_never_publish() {
        let (_dir, app, diag, target) = fixture();
        assert!(export_with(&app, &diag, &target, || Err("no current writer")).is_err());
        assert!(export_with(&app, &diag, &target, || { let mut s = snapshot(); s.incomplete = true; Ok(s) }).is_err());
        std::fs::write(&diag, b"old snapshot only").unwrap();
        assert!(export_with(&app, &diag, &target, || Ok(snapshot())).is_err());
        std::fs::remove_file(&diag).unwrap();
        assert!(export_with(&app, &diag, &target, || panic!("missing file cannot capture")).is_err());
        assert_eq!(std::fs::read(target).unwrap(), b"preserve");
        assert_eq!(std::fs::read(app).unwrap(), b"ordinary\n");
    }
    #[test]
    fn source_targets_and_canonical_aliases_are_rejected_without_capture() {
        let (dir, app, diag, _target) = fixture();
        let alias = dir.path().join(".").join("openless.log");
        for target in [&app, &diag, &alias] {
            assert!(export_with(&app, &diag, target, || panic!("alias cannot capture")).is_err());
        }
        assert_eq!(std::fs::read(app).unwrap(), b"ordinary\n");
    }
    #[cfg(unix)]
    #[test]
    fn symlink_to_either_input_is_rejected_without_capture() {
        let (dir, app, diag, _) = fixture();
        for (index, source) in [&app, &diag].into_iter().enumerate() {
            let target = dir.path().join(format!("alias-{index}"));
            std::os::unix::fs::symlink(source, &target).unwrap();
            assert!(export_with(&app, &diag, &target, || panic!("alias cannot capture")).is_err());
        }
    }
    #[cfg(target_os = "windows")]
    #[test]
    fn windows_canonical_path_comparison_is_case_insensitive() {
        assert!(same_path(Path::new(r"C:\OpenLess\Logs\openless.log"),
                          Path::new(r"c:\openless\logs\OPENLESS.LOG")).unwrap());
    }
    #[test]
    fn hardlink_target_is_replaced_without_truncating_source() {
        let (dir, app, diag, _) = fixture();
        let link = dir.path().join("linked.log");
        std::fs::hard_link(&app, &link).unwrap();
        export_with(&app, &diag, &link, || Ok(snapshot())).unwrap();
        assert_eq!(std::fs::read(app).unwrap(), b"ordinary\n");
        assert!(std::fs::read_to_string(link).unwrap().contains("current"));
    }
    #[test]
    fn actual_production_encoder_checkpoint_and_unknowns_reach_the_single_delivered_log() {
        let (_dir, app, diag, target) = fixture();
        let (bytes, snapshot) = sink::export_fixture();
        std::fs::write(&diag, bytes).unwrap();
        export_with(&app, &diag, &target, || Ok(snapshot)).unwrap();
        let out = std::fs::read_to_string(target).unwrap();
        let value: serde_json::Value = out.lines().filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .find(|v| v["schema"] == "issue1338-export-checkpoint.v1").unwrap();
        assert_eq!(value["run_id"], "r1338");
        assert_eq!(value["capture_health"]["run_id"], "r1338");
        assert_eq!(value["capture_health"]["dropped"], 0);
        assert_eq!(value["frontend_coverage"], serde_json::json!({
            "scope": "backend_only", "request_intent": "unknown", "request_send": "unknown",
            "response_application": "unknown", "transport_loss": "unknown"
        }));
        assert_eq!(value["tail_capture"], "unknown");
        assert_eq!(value["historical_runs"], "included_not_current_coverage_unknown");
        assert_eq!(value["feature_enabled"], super::super::enabled());
    }
    #[test]
    fn failed_publish_preserves_sources_and_existing_target() {
        let (dir, app, diag, _) = fixture();
        let target = dir.path().join("directory-target");
        std::fs::create_dir(&target).unwrap();
        assert!(export_with(&app, &diag, &target, || Ok(snapshot())).is_err());
        assert!(target.is_dir());
        assert_eq!(std::fs::read(app).unwrap(), b"ordinary\n");
        assert_eq!(std::fs::read(diag).unwrap().len() as u64, snapshot().end);
    }
}
