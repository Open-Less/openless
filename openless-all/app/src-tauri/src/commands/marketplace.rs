use super::*;
use std::io::Write;

fn command_error(error: openless_core::BackendError) -> String {
    error.to_string()
}

#[tauri::command]
pub async fn marketplace_list(
    core: CoreState<'_>,
    query: Option<String>,
    sort: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<openless_core::MarketplaceListItem>, String> {
    core
        .services()
        .marketplace
        .list(openless_core::MarketplaceQuery { query, sort, limit })
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn marketplace_detail(
    core: CoreState<'_>,
    pack_id: String,
) -> Result<openless_core::MarketplaceDetail, String> {
    core
        .services()
        .marketplace
        .detail(pack_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn marketplace_install(
    core: CoreState<'_>,
    pack_id: String,
) -> Result<StylePack, String> {
    core
        .services()
        .marketplace
        .install(pack_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn marketplace_download(
    core: CoreState<'_>,
    pack_id: String,
    target_path: String,
) -> Result<(), String> {
    if target_path.trim().is_empty() {
        return Err("marketplace download target is empty".into());
    }
    let bytes = core
        .services()
        .marketplace
        .download_archive(pack_id)
        .await
        .map_err(command_error)?;
    write_marketplace_archive_target(&target_path, &bytes)
}

#[tauri::command]
pub async fn marketplace_upload(
    core: CoreState<'_>,
    pack_id: String,
    origin_pack_id: Option<String>,
) -> Result<openless_core::MarketplaceUploadResult, String> {
    core
        .services()
        .marketplace
        .upload(pack_id, origin_pack_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn marketplace_like(
    core: CoreState<'_>,
    pack_id: String,
) -> Result<openless_core::MarketplaceLikeResult, String> {
    core
        .services()
        .marketplace
        .toggle_like(pack_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn marketplace_delete(
    core: CoreState<'_>,
    pack_id: String,
) -> Result<(), String> {
    core
        .services()
        .marketplace
        .delete(pack_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn marketplace_my_likes(core: CoreState<'_>) -> Result<Vec<String>, String> {
    core
        .services()
        .marketplace
        .my_likes()
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn marketplace_my_packs(
    core: CoreState<'_>,
) -> Result<Vec<openless_core::MarketplaceMyPackItem>, String> {
    core
        .services()
        .marketplace
        .my_packs()
        .await
        .map_err(command_error)
}

fn write_marketplace_archive_target(target_path: &str, bytes: &[u8]) -> Result<(), String> {
    #[cfg(target_os = "android")]
    if target_path.starts_with("content://") {
        return crate::android::jni::android::write_content_uri(target_path, bytes)
            .map_err(|_| "write marketplace archive target failed".to_string());
    }

    if target_path.starts_with("content://") {
        return Err("content URI targets are only supported on Android".to_string());
    }
    if target_path.starts_with("file://") {
        return Err(
            "file URI targets are not supported; provide a filesystem path instead".to_string(),
        );
    }
    if target_path.trim().is_empty() {
        return Err("marketplace download target is empty".to_string());
    }
    let path = std::path::Path::new(target_path);
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).map_err(|error| {
            format!("create marketplace archive target directory failed: {error}")
        })?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .map_err(|error| format!("create marketplace archive target failed: {error}"))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("write marketplace archive target failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::write_marketplace_archive_target;

    #[test]
    fn filesystem_archive_sink_preserves_validated_core_bytes() {
        let root = std::env::temp_dir().join(format!(
            "openless-marketplace-host-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let target = root.join("downloaded.zip");
        write_marketplace_archive_target(&target.to_string_lossy(), b"validated archive bytes")
            .unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"validated archive bytes");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn filesystem_archive_sink_rejects_uri_confusion() {
        assert!(
            write_marketplace_archive_target("file:///tmp/archive.zip", b"bytes")
                .unwrap_err()
                .contains("file URI")
        );
        #[cfg(not(target_os = "android"))]
        assert!(
            write_marketplace_archive_target("content://archive", b"bytes")
                .unwrap_err()
                .contains("only supported on Android")
        );
    }
}
