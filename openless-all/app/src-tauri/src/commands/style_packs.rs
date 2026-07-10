use super::*;

fn refresh_tray_menu_async(app: &AppHandle) {
    let app_for_main = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Err(err) = crate::refresh_tray_microphone_menu(&app_for_main) {
            log::warn!("[tray] refresh after style change failed: {err}");
        }
    });
}

fn emit_prefs_changed(app: &AppHandle, prefs: &UserPreferences) {
    let _ = app.emit("prefs:changed", prefs);
    let _ = app.emit_to("main", "prefs:changed", prefs);
}

pub(crate) fn sync_style_pack_prefs_and_persist(
    coord: &Coordinator,
    app: &AppHandle,
    mut prefs: UserPreferences,
) -> Result<UserPreferences, String> {
    let packs = coord.style_packs().list().map_err(|e| e.to_string())?;
    sync_style_pack_preferences(&mut prefs, &packs);
    coord
        .prefs()
        .set(prefs.clone())
        .map_err(|e| e.to_string())?;
    emit_prefs_changed(app, &prefs);
    refresh_tray_menu_async(app);
    Ok(prefs)
}

pub(crate) fn activate_style_pack_by_id(
    coord: &Coordinator,
    app: &AppHandle,
    id: &str,
) -> Result<StylePack, String> {
    let mut prefs = coord.prefs().get();
    let pack = coord.style_packs().get(id).map_err(|e| e.to_string())?;
    log::info!(
        "[style-pack] activate helper requested id={} kind={:?} base_mode={:?} enabled={}",
        pack.id,
        pack.kind,
        pack.base_mode,
        pack.enabled
    );
    if !pack.enabled {
        coord
            .style_packs()
            .set_enabled(id, true)
            .map_err(|e| e.to_string())?;
    }
    prefs.active_style_pack_id = id.to_string();
    sync_style_pack_prefs_and_persist(coord, app, prefs)?;
    log::info!("[style-pack] activate helper applied id={id}");
    coord
        .style_packs()
        .get(id)
        .map(|mut pack| {
            pack.active = true;
            pack
        })
        .map_err(|e| e.to_string())
}

pub(crate) fn activate_builtin_style_mode(
    coord: &Coordinator,
    app: &AppHandle,
    mode: PolishMode,
) -> Result<(), String> {
    let pack_id = builtin_style_pack_id(mode).to_string();
    log::info!(
        "[style-pack] activate builtin mode helper mode={:?} pack_id={}",
        mode,
        pack_id
    );
    let _ = activate_style_pack_by_id(coord, app, &pack_id)?;
    Ok(())
}

// ─────────────────────────── style packs ───────────────────────────

#[tauri::command]
pub fn list_style_packs(coord: CoordinatorState<'_>) -> Result<Vec<StylePack>, String> {
    let prefs = coord.prefs().get();
    coord
        .style_packs()
        .list_with_active(&prefs.active_style_pack_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_style_pack_from_template(
    coord: CoordinatorState<'_>,
    app: AppHandle,
    template: StylePack,
) -> Result<StylePack, String> {
    log::info!(
        "[style-pack] command create_from_template name={} base_mode={:?}",
        template.name,
        template.base_mode
    );
    let created = coord
        .style_packs()
        .create_from_template(template)
        .map_err(|e| e.to_string())?;
    let prefs = coord.prefs().get();
    let _ = sync_style_pack_prefs_and_persist(&*coord, &app, prefs)?;
    Ok(created)
}

#[tauri::command]
pub fn save_style_pack(
    coord: CoordinatorState<'_>,
    app: AppHandle,
    style_pack: StylePack,
) -> Result<StylePack, String> {
    log::info!(
        "[style-pack] command save id={} kind={:?} base_mode={:?}",
        style_pack.id,
        style_pack.kind,
        style_pack.base_mode
    );
    let saved = coord
        .style_packs()
        .upsert(style_pack)
        .map_err(|e| e.to_string())?;
    if saved.kind == StylePackKind::Builtin {
        let prefs = coord.prefs().get();
        let _ = sync_style_pack_prefs_and_persist(&*coord, &app, prefs)?;
    }
    Ok(saved)
}

#[tauri::command]
pub fn preview_style_pack_runtime(
    coord: CoordinatorState<'_>,
    style_pack: StylePack,
) -> Result<StylePackRuntimeDiagnostics, String> {
    log::info!(
        "[style-pack] command preview_runtime id={} base_mode={:?} prompt_chars={}",
        style_pack.id,
        style_pack.base_mode,
        style_pack.prompt.chars().count()
    );
    Ok(coord.preview_style_pack_runtime(&style_pack))
}

#[tauri::command]
pub fn set_active_style_pack(
    coord: CoordinatorState<'_>,
    app: AppHandle,
    id: String,
) -> Result<StylePack, String> {
    activate_style_pack_by_id(&coord, &app, &id)
}

#[tauri::command]
pub fn set_style_pack_enabled(
    coord: CoordinatorState<'_>,
    app: AppHandle,
    id: String,
    enabled: bool,
) -> Result<Vec<StylePack>, String> {
    log::info!(
        "[style-pack] command set_enabled requested id={} enabled={}",
        id,
        enabled
    );
    coord
        .style_packs()
        .set_enabled(&id, enabled)
        .map_err(|e| e.to_string())?;
    let mut prefs = coord.prefs().get();
    if !enabled && prefs.active_style_pack_id == id {
        prefs.active_style_pack_id = default_active_style_pack_id();
    }
    let prefs = sync_style_pack_prefs_and_persist(&*coord, &app, prefs)?;
    coord
        .style_packs()
        .list_with_active(&prefs.active_style_pack_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn reset_builtin_style_pack(
    coord: CoordinatorState<'_>,
    app: AppHandle,
    id: String,
) -> Result<StylePack, String> {
    log::info!("[style-pack] command reset_builtin requested id={id}");
    let saved = coord
        .style_packs()
        .reset_builtin(&id)
        .map_err(|e| e.to_string())?;
    let prefs = coord.prefs().get();
    let _ = sync_style_pack_prefs_and_persist(&*coord, &app, prefs)?;
    Ok(saved)
}

#[tauri::command]
pub fn delete_style_pack(
    coord: CoordinatorState<'_>,
    app: AppHandle,
    id: String,
) -> Result<(), String> {
    let mut prefs = coord.prefs().get();
    log::info!("[style-pack] command delete requested id={id}");
    coord
        .style_packs()
        .remove_imported(&id)
        .map_err(|e| e.to_string())?;
    if prefs.active_style_pack_id == id {
        prefs.active_style_pack_id = default_active_style_pack_id();
        let _ = sync_style_pack_prefs_and_persist(&*coord, &app, prefs)?;
    } else {
        refresh_tray_menu_async(&app);
    }
    Ok(())
}

#[tauri::command]
pub fn import_style_pack_from_zip(
    coord: CoordinatorState<'_>,
    zip_path: String,
) -> Result<StylePack, String> {
    log::info!("[style-pack] command import requested zip_path={zip_path}");
    coord
        .style_packs()
        .import_from_zip(std::path::Path::new(&zip_path))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn export_style_pack_to_zip(
    coord: CoordinatorState<'_>,
    id: String,
    target_path: String,
) -> Result<String, String> {
    log::info!(
        "[style-pack] command export requested id={} target_path={}",
        id,
        target_path
    );
    coord
        .style_packs()
        .export_to_zip(&id, std::path::Path::new(&target_path))
        .map_err(|e| e.to_string())?;
    Ok(target_path)
}

// ─────────────────────────── style toggles (compat) ───────────────────────────

#[tauri::command]
pub fn set_default_polish_mode(
    coord: CoordinatorState<'_>,
    app: AppHandle,
    mode: PolishMode,
) -> Result<(), String> {
    activate_builtin_style_mode(&coord, &app, mode)
}

#[tauri::command]
pub fn set_style_enabled(
    coord: CoordinatorState<'_>,
    app: AppHandle,
    mode: PolishMode,
    enabled: bool,
) -> Result<(), String> {
    let pack_id = builtin_style_pack_id(mode).to_string();
    log::info!(
        "[style-pack] compat set_style_enabled mode={:?} pack_id={} enabled={}",
        mode,
        pack_id,
        enabled
    );
    coord
        .style_packs()
        .set_enabled(&pack_id, enabled)
        .map_err(|e| e.to_string())?;
    let mut prefs = coord.prefs().get();
    if !enabled && prefs.active_style_pack_id == pack_id {
        prefs.active_style_pack_id = default_active_style_pack_id();
    }
    let _ = sync_style_pack_prefs_and_persist(&*coord, &app, prefs)?;
    Ok(())
}
