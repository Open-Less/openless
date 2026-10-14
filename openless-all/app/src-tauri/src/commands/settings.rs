use super::*;

#[tauri::command]
pub fn get_settings(core: CoreState<'_>) -> UserPreferences {
    let mut prefs = core.get_preferences();
    prefs.update_channel = effective_update_channel(None, &prefs, env!("CARGO_PKG_VERSION"));
    sync_low_latency_capture_preferences(&prefs);
    prefs
}

#[tauri::command]
pub fn get_default_style_system_prompts() -> StyleSystemPrompts {
    StyleSystemPrompts::default()
}

struct TauriSettingsRuntime<'a> {
    coord: &'a Coordinator,
}

fn settings_save_error(error: openless_core::BackendError) -> String {
    let mut message = error.message;
    if let Some(failures) = error
        .details
        .as_ref()
        .and_then(|details| details.get("compensationErrors"))
        .and_then(serde_json::Value::as_array)
    {
        for failure in failures {
            if let Some(reason) = failure.get("message").and_then(serde_json::Value::as_str) {
                message.push_str(&format!("; rollback failed: {reason}"));
            }
        }
    }
    message
}

impl<'a> TauriSettingsRuntime<'a> {
    fn new(coord: &'a Coordinator) -> Self {
        Self { coord }
    }

    fn platform_error(message: impl Into<String>) -> openless_core::BackendError {
        openless_core::BackendError::new(openless_core::BackendErrorCode::Platform, message)
    }

    fn apply_windows_keyboard(
        &self,
        target: &openless_core::WindowsKeyboardRuntimeTarget,
    ) -> Result<(), openless_core::BackendError> {
        crate::windows_ime_profile::apply_windows_openless_keyboard_list(
            target.openless_language_profile_enabled,
        )
        .map_err(Self::platform_error)
    }

    fn apply_hotkeys(
        &self,
        change: &openless_core::SettingsValueChange<openless_core::HotkeyRuntimeTarget>,
    ) -> Result<(), openless_core::BackendError> {
        self.coord
            .apply_hotkey_runtime_change(change)
            .map_err(Self::platform_error)
    }
}

impl openless_core::SettingsRuntime for TauriSettingsRuntime<'_> {
    fn prepare(
        &self,
        plan: &openless_core::SettingsEffectPlan,
    ) -> Result<openless_core::SettingsEffectReceipt, openless_core::SettingsEffectFailure> {
        let mut receipt = openless_core::SettingsEffectReceipt::default();
        if let Some(change) = &plan.windows_keyboard {
            if let Err(error) = self.apply_windows_keyboard(&change.next) {
                return Err(openless_core::SettingsEffectFailure::after_side_effect(
                    error, receipt,
                ));
            }
            receipt
                .applied
                .push(openless_core::SettingsEffectKind::WindowsKeyboard);
        }
        if let Some(change) = &plan.active_asr_provider {
            if let Err(error) =
                sync_active_asr_provider_to_vault(&change.next).map_err(Self::platform_error)
            {
                return Err(openless_core::SettingsEffectFailure::after_side_effect(
                    error, receipt,
                ));
            }
            receipt
                .applied
                .push(openless_core::SettingsEffectKind::ActiveAsrProvider);
        }
        Ok(receipt)
    }

    fn commit(
        &self,
        plan: &openless_core::SettingsEffectPlan,
        receipt: &mut openless_core::SettingsEffectReceipt,
    ) -> Result<(), openless_core::SettingsEffectFailure> {
        let Some(change) = &plan.hotkeys else {
            return Ok(());
        };
        if !receipt
            .applied
            .contains(&openless_core::SettingsEffectKind::Hotkeys)
        {
            receipt
                .applied
                .push(openless_core::SettingsEffectKind::Hotkeys);
        }
        self.apply_hotkeys(change).map_err(|error| {
            openless_core::SettingsEffectFailure::after_side_effect(error, receipt.clone())
        })
    }

    fn restore(
        &self,
        plan: &openless_core::SettingsEffectPlan,
        receipt: &openless_core::SettingsEffectReceipt,
    ) -> Result<(), openless_core::BackendError> {
        let mut failures = Vec::new();
        for effect in receipt.applied.iter().rev() {
            let result = match effect {
                openless_core::SettingsEffectKind::Hotkeys => plan
                    .hotkeys
                    .as_ref()
                    .map(|change| {
                        let reverse = openless_core::SettingsValueChange {
                            previous: change.next.clone(),
                            next: change.previous.clone(),
                        };
                        self.apply_hotkeys(&reverse)
                    })
                    .unwrap_or(Ok(())),
                openless_core::SettingsEffectKind::ActiveAsrProvider => plan
                    .active_asr_provider
                    .as_ref()
                    .map(|change| {
                        sync_active_asr_provider_to_vault(&change.previous)
                            .map_err(Self::platform_error)
                    })
                    .unwrap_or(Ok(())),
                // Desktop launch-at-login is owned by tauri-plugin-autostart;
                // Core currently does not stage this preference on Tauri.
                openless_core::SettingsEffectKind::LaunchAtLogin => Ok(()),
                openless_core::SettingsEffectKind::WindowsKeyboard => plan
                    .windows_keyboard
                    .as_ref()
                    .map(|change| self.apply_windows_keyboard(&change.previous))
                    .unwrap_or(Ok(())),
            };
            if let Err(error) = result {
                failures.push(error.message);
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(Self::platform_error(format!(
                "failed to restore settings runtime: {}",
                failures.join("; ")
            )))
        }
    }
}

fn persist_settings_with_host_lock_held(
    coord: &Coordinator,
    prefs: UserPreferences,
) -> Result<(), String> {
    coord
        .backend()
        .update_settings(
            prefs,
            openless_core::SettingsUpdateOptions::SETTINGS_DOCUMENT,
            &TauriSettingsRuntime::new(coord),
        )
        .map(|outcome| {
            if outcome.reconciled_hotkey_count > 0 {
                log::warn!(
                    "[settings] 热键冲突已自动化解（调整 {} 项）后保存",
                    outcome.reconciled_hotkey_count
                );
            }
        })
        .map_err(settings_save_error)
}

fn persist_settings_preserving_update_channel(
    coord: &Coordinator,
    mut prefs: UserPreferences,
) -> Result<(), String> {
    let _host_guard = coord.lock_settings_host();
    // Read and backfill under the same write lock, so a concurrent channel switch can't be
    // overwritten by a stale settings snapshot.
    preserve_update_channel_preferences(&mut prefs, &coord.backend().get_preferences());
    persist_settings_with_host_lock_held(coord, prefs)
}

pub(crate) fn persist_strict_settings(
    coord: &Coordinator,
    mut prefs: UserPreferences,
) -> Result<(), String> {
    let _host_guard = coord.lock_settings_host();
    preserve_update_channel_preferences(&mut prefs, &coord.backend().get_preferences());
    coord
        .backend()
        .update_settings(
            prefs,
            openless_core::SettingsUpdateOptions::STRICT,
            &TauriSettingsRuntime::new(coord),
        )
        .map(|_| {
            sync_low_latency_capture_preferences(&coord.backend().get_preferences());
        })
        .map_err(settings_save_error)
}

#[cfg(target_os = "windows")]
fn sync_low_latency_capture_preferences(prefs: &UserPreferences) {
    crate::recorder::warm_capture_hub().configure(
        prefs.low_latency_dictation_enabled,
        (!prefs.microphone_device_name.trim().is_empty())
            .then(|| prefs.microphone_device_name.clone()),
    );
}

#[cfg(not(target_os = "windows"))]
fn sync_low_latency_capture_preferences(_prefs: &UserPreferences) {}

async fn invalidate_llm_tests_if_thinking_changed(
    coord: &Coordinator,
    previous: &UserPreferences,
    next: &UserPreferences,
) -> Result<(), String> {
    if previous.llm_thinking_enabled != next.llm_thinking_enabled {
        coord
            .backend()
            .invalidate_channel_tests(openless_core::ChannelKind::Llm)
            .await
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(not(mobile))]
#[tauri::command]
pub async fn set_settings(
    coord: CoordinatorState<'_>,
    app: AppHandle,
    mut prefs: UserPreferences,
    edits: Option<std::collections::BTreeMap<String, serde_json::Value>>,
) -> Result<UserPreferences, String> {
    // Capture old values for the remote-input service diff (start/stop/restart when the
    // port/switch changes after persist).
    let remote_prev = coord.backend().get_preferences();
    if let Some(edits) = &edits {
        prefs = openless_core::preference_patch::patch_preferences(&prefs, edits)
            .map_err(|e| e.to_string())?;
    }
    let packs = coord
        .backend()
        .list_style_packs(&prefs.active_style_pack_id)
        .map_err(|e| e.to_string())?;
    sync_style_pack_preferences(&mut prefs, &packs);
    prefs.android_overlay_trigger = prefs.android_overlay_trigger.normalized();
    invalidate_llm_tests_if_thinking_changed(&coord, &remote_prev, &prefs).await?;
    // Persist changed fields and notify every WebView through the shared settings path.
    if let Some(edits) = edits {
        persist_setting_fields(&coord, &edits)?;
    } else {
        persist_settings_preserving_update_channel(&coord, prefs)?;
    }
    let prefs = coord.backend().get_preferences();
    sync_low_latency_capture_preferences(&prefs);
    // Sync the capsule-style atom on save: the next recording's entrance frame carries the
    // new style instead of depending on emit_capsule's ~30Hz main-thread closure sync (a
    // congested Windows main thread delays the closure → the whole session shows the old
    // style). The frontend also receives the new style via the prefs:changed broadcast,
    // giving instant reskin mid-recording.
    coord.sync_capsule_style_from_preferences();
    // Rebuild the client connection pool immediately when the system-proxy switch changes (issue #869).
    if remote_prev.use_system_proxy != prefs.use_system_proxy {
        crate::net::set_use_system_proxy(prefs.use_system_proxy);
    }
    #[cfg(target_os = "android")]
    coord.apply_android_overlay_settings_change(&remote_prev, &prefs);
    // refresh_tray_microphone_menu calls NSStatusItem.set_menu internally and must run on
    // the main thread. set_settings is an async Tauri command and is not on the macOS UI
    // main thread while executing; calling it directly from here trips the macOS main-thread
    // assertion or deadlocks the dispatch queue, freezing the whole UI (the root cause of
    // every keypress dead after a user preference change). Dispatch to the main thread and
    // continue; the async task does not block.
    let app_for_main = app.clone();
    let prefs_for_main = prefs.clone();
    let _ = app.run_on_main_thread(move || {
        if let Err(err) = crate::refresh_tray_microphone_menu(&app_for_main) {
            log::warn!("[tray] refresh microphone menu after settings save failed: {err}");
            let tray_state = app_for_main.state::<TrayMicrophoneMenuState>();
            sync_tray_microphone_selection(
                &tray_state.lock(),
                &prefs_for_main.microphone_device_name,
            );
        }
    });
    // Remote input: start/stop or restart the service when the switch/port changes
    // (PIN changes go through the regenerate_remote_pin command).
    if remote_prev.remote_input_enabled != prefs.remote_input_enabled
        || remote_prev.remote_input_port != prefs.remote_input_port
    {
        coord
            .backend()
            .services()
            .remote_input
            .configure(openless_core::RemoteInputConfig {
                enabled: prefs.remote_input_enabled,
                port: prefs.remote_input_port,
            })
            .await
            .map_err(|error| error.message)?;
    }
    Ok(prefs)
}

#[cfg(mobile)]
#[tauri::command]
pub async fn set_settings(
    coord: CoordinatorState<'_>,
    mut prefs: UserPreferences,
    edits: Option<std::collections::BTreeMap<String, serde_json::Value>>,
) -> Result<UserPreferences, String> {
    let previous = coord.backend().get_preferences();
    if let Some(edits) = &edits {
        prefs = openless_core::preference_patch::patch_preferences(&prefs, edits)
            .map_err(|e| e.to_string())?;
    }
    let packs = coord
        .backend()
        .list_style_packs(&prefs.active_style_pack_id)
        .map_err(|e| e.to_string())?;
    sync_style_pack_preferences(&mut prefs, &packs);
    prefs.android_overlay_trigger = prefs.android_overlay_trigger.normalized();
    invalidate_llm_tests_if_thinking_changed(&coord, &previous, &prefs).await?;
    if let Some(edits) = edits {
        persist_setting_fields(&coord, &edits)?;
    } else {
        persist_settings_preserving_update_channel(&coord, prefs)?;
    }
    let prefs = coord.backend().get_preferences();
    sync_low_latency_capture_preferences(&prefs);
    // Sync the capsule-style atom on save (same source as the Android notification capsule
    // payload; see emit_capsule).
    coord.sync_capsule_style_from_preferences();
    // Rebuild the client connection pool immediately when the system-proxy switch changes (issue #869).
    if previous.use_system_proxy != prefs.use_system_proxy {
        crate::net::set_use_system_proxy(prefs.use_system_proxy);
    }
    #[cfg(target_os = "android")]
    coord.apply_android_overlay_settings_change(&previous, &prefs);
    // Do not emit "prefs:changed" here directly: coord.backend().update_settings()
    // (called via persist_settings_preserving_update_channel above) already fires
    // a BackendEventKind::PreferencesChanged event that tauri_events.rs relays to
    // every webview, including Android's (mobile_runtime.rs wires up
    // tauri_events::start()). An inline emit here would just double-broadcast.
    Ok(prefs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_save_reports_registration_and_rollback_failure() {
        let mut error = openless_core::BackendError::new(
            openless_core::BackendErrorCode::Platform,
            "hook installation failed",
        );
        error.details = Some(serde_json::json!({
            "compensationErrors": [{"message": "old binding restore failed"}]
        }));
        assert_eq!(
            settings_save_error(error),
            "hook installation failed; rollback failed: old binding restore failed"
        );
    }

    #[test]
    fn settings_save_preserves_current_style_preferences_before_write() {
        let packs = crate::types::builtin_style_packs();
        let current = UserPreferences {
            default_mode: PolishMode::Light,
            active_style_pack_id: builtin_style_pack_id(PolishMode::Light).to_string(),
            ..UserPreferences::default()
        };
        let mut stale_settings_payload = UserPreferences {
            default_mode: PolishMode::Formal,
            active_style_pack_id: builtin_style_pack_id(PolishMode::Formal).to_string(),
            ..UserPreferences::default()
        };

        stale_settings_payload.preserve_style_preferences_from(&current);
        sync_style_pack_preferences(&mut stale_settings_payload, &packs);

        assert_eq!(
            stale_settings_payload.active_style_pack_id,
            builtin_style_pack_id(PolishMode::Light)
        );
        assert_eq!(stale_settings_payload.default_mode, PolishMode::Light);
    }

    #[test]
    fn update_channel_defaults_to_build_channel_until_user_selects_one() {
        let mut prefs = UserPreferences::default();

        assert_eq!(
            effective_update_channel(None, &prefs, "2.0.0-Beta.1"),
            UpdateChannel::Beta
        );
        assert_eq!(
            effective_update_channel(None, &prefs, "2.0.0"),
            UpdateChannel::Stable
        );
        let legacy_beta = UserPreferences {
            update_channel: UpdateChannel::Beta,
            update_channel_explicit: true,
            ..UserPreferences::default()
        };
        assert_eq!(
            effective_update_channel(None, &legacy_beta, "2.0.0"),
            UpdateChannel::Beta
        );
        assert_eq!(
            effective_update_channel(Some(UpdateChannel::Stable), &prefs, "2.0.0-Beta.1"),
            UpdateChannel::Stable
        );

        assert!(select_update_channel(&mut prefs, UpdateChannel::Stable));
        assert!(prefs.update_channel_explicit);
        assert_eq!(
            effective_update_channel(None, &prefs, "2.0.0-Beta.1"),
            UpdateChannel::Stable
        );
        assert!(!select_update_channel(&mut prefs, UpdateChannel::Stable));
        assert!(select_update_channel(&mut prefs, UpdateChannel::Beta));
        assert_eq!(prefs.update_channel, UpdateChannel::Beta);
        assert!(prefs.update_channel_explicit);
    }

    #[test]
    fn general_settings_save_preserves_dedicated_update_channel_fields() {
        let current = UserPreferences {
            update_channel: UpdateChannel::Stable,
            update_channel_explicit: true,
            ..UserPreferences::default()
        };
        let mut stale_payload = UserPreferences {
            update_channel: UpdateChannel::Beta,
            update_channel_explicit: false,
            ..UserPreferences::default()
        };

        preserve_update_channel_preferences(&mut stale_payload, &current);

        assert_eq!(stale_payload.update_channel, UpdateChannel::Stable);
        assert!(stale_payload.update_channel_explicit);
    }
}

// ─────────────────────────── release channel (Beta opt-in) ───────────────────────────
//
// Channel preference writes reuse persist_settings (as set_settings does) so hotkey
// fallback normalization stays consistent with other prefs writes, and "prefs:changed" is
// emitted afterwards to sync the frontend across webviews.
//
// Update: plugin-updater 2.10.1's Builder now exposes the .endpoints() runtime API (the
// "unsupported" note recorded in CLAUDE.md no longer holds). This section, together with
// the `app_check_update_with_channel` command, implements Beta auto-update: Stable channel →
// tauri.conf's default endpoints; Beta channel → fetch_latest_beta_release gets the latest
// prerelease tag → build the -beta manifest URL →
// builder.endpoints(vec![url]).build().check(). Stable users can never hit a Beta package
// (Beta-tag manifests carry a `-beta` filename suffix, physically separated from Stable
// manifests in the GitHub Release assets).

fn effective_update_channel(
    requested: Option<UpdateChannel>,
    prefs: &UserPreferences,
    app_version: &str,
) -> UpdateChannel {
    requested.unwrap_or_else(|| {
        if prefs.update_channel_explicit {
            prefs.update_channel
        } else if app_version.contains('-') {
            UpdateChannel::Beta
        } else {
            UpdateChannel::Stable
        }
    })
}

fn select_update_channel(prefs: &mut UserPreferences, channel: UpdateChannel) -> bool {
    let changed = prefs.update_channel != channel || !prefs.update_channel_explicit;
    prefs.update_channel = channel;
    prefs.update_channel_explicit = true;
    changed
}

fn preserve_update_channel_preferences(incoming: &mut UserPreferences, current: &UserPreferences) {
    incoming.update_channel = current.update_channel;
    incoming.update_channel_explicit = current.update_channel_explicit;
}

#[tauri::command]
pub fn get_update_channel(core: CoreState<'_>) -> UpdateChannel {
    let prefs = core.get_preferences();
    effective_update_channel(None, &prefs, env!("CARGO_PKG_VERSION"))
}

#[tauri::command]
pub fn set_update_channel(
    coord: CoordinatorState<'_>,
    channel: UpdateChannel,
) -> Result<(), String> {
    // Channel read and persistence must share one write critical section, so they can't be
    // overwritten in reverse by concurrent regular settings writes.
    let _host_guard = coord.lock_settings_host();
    let mut prefs = coord.backend().get_preferences();
    if !select_update_channel(&mut prefs, channel) {
        return Ok(());
    }
    persist_settings_with_host_lock_held(&*coord, prefs)?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatestBetaRelease {
    pub tag_name: String,
    pub html_url: String,
    pub published_at: String,
}

/// Fetches the GitHub Releases atom feed to find the latest Beta release.
///
/// History: this used the `api.github.com/repos/.../releases` REST endpoint, which allows
/// only **60 unauthenticated req/h/IP**; many users toggling Beta repeatedly easily hit the
/// 403 rate limit (the reported "failed to get Beta version info"). `releases.atom` is a
/// public page with CDN cache and no equivalent rate limit.
/// The Atom feed doesn't explicitly mark prereleases, so filter by the current
/// `-Beta.N-tauri` convention while also accepting the historical `-beta-tauri` suffix.
///
/// `Ok(None)` = no Beta release published yet; `Err(String)` = network/parse failure.
#[tauri::command]
pub async fn fetch_latest_beta_release() -> Result<Option<LatestBetaRelease>, String> {
    let resp = net::send_with_retry(|| {
        net::http()
            .get("https://github.com/Open-Less/openless/releases.atom")
            .timeout(std::time::Duration::from_secs(15))
    })
    .await
    .map_err(|e| format!("fetch releases.atom: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("releases.atom status {}", resp.status()));
    }
    let body = resp
        .text()
        .await
        .map_err(|e| format!("read atom body: {e}"))?;
    Ok(parse_latest_beta_from_atom(&body))
}

/// Minimal string parsing of the atom feed to avoid an XML dependency. Each
/// `<entry>...</entry>` contains one
/// `<link rel="alternate" type="text/html" href=".../releases/tag/<tag>"/>` line; grab the
/// tag via the unique `/releases/tag/` anchor.
pub(crate) fn parse_latest_beta_from_atom(body: &str) -> Option<LatestBetaRelease> {
    for entry in body.split("<entry>").skip(1) {
        let entry_body = entry
            .split_once("</entry>")
            .map(|(b, _)| b)
            .unwrap_or(entry);
        let needle = "/releases/tag/";
        let tag_start = match entry_body.find(needle) {
            Some(i) => i + needle.len(),
            None => continue,
        };
        let tag_after = &entry_body[tag_start..];
        let tag_end = tag_after
            .find(|c: char| c == '"' || c == '<' || c == ' ' || c == '/')
            .unwrap_or(tag_after.len());
        let tag_name = tag_after[..tag_end].to_string();
        if !is_beta_release_tag(&tag_name) {
            continue;
        }
        let html_url = format!("https://github.com/Open-Less/openless/releases/tag/{tag_name}");
        let published_at =
            extract_between(entry_body, "<updated>", "</updated>").unwrap_or_default();
        return Some(LatestBetaRelease {
            tag_name,
            html_url,
            published_at,
        });
    }
    None
}

fn is_beta_release_tag(tag_name: &str) -> bool {
    if tag_name.ends_with("-beta-tauri") {
        return true;
    }

    let Some((version, beta_number)) = tag_name
        .strip_prefix('v')
        .and_then(|tag| tag.strip_suffix("-tauri"))
        .and_then(|tag| tag.split_once("-Beta."))
    else {
        return false;
    };

    if beta_number.is_empty() || !beta_number.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }

    let mut version_parts = version.split('.');
    (0..3).all(|_| {
        version_parts
            .next()
            .is_some_and(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
    }) && version_parts.next().is_none()
}

fn extract_between(haystack: &str, open: &str, close: &str) -> Option<String> {
    let start = haystack.find(open)? + open.len();
    let end = haystack[start..].find(close)?;
    Some(haystack[start..start + end].to_string())
}

// ─────────────────────── Channel-aware updater check ────────────────────────
//
// Replaces the frontend's previous direct import('@tauri-apps/plugin-updater').check() path:
// - Stable channel: the builder keeps tauri.conf's stable manifest URL endpoints untouched.
// - Beta channel: fetch_latest_beta_release gets the latest prerelease tag, builds the -beta
//   manifest URL (as a mirror + direct pair), then
//   builder.endpoints(vec![url])?.build()?.check().
//
// The returned Metadata shape exactly matches plugin-updater's JS UpdateMetadata (rid +
// currentVersion and other camelCase fields), so the frontend can call
// `new Update(metadata)` directly and reuse the plugin's download / install / close
// implementations instead of writing our own download and signature verification.
//
// Physical isolation: manifests published from Beta tags carry a `-beta` filename suffix
// (see the comment at line 382 of release-tauri.yml), separate from Stable's
// `latest-{tgt}-{arch}.json` in the GitHub Release assets — even if buggy code passed a
// Beta URL to a Stable user, HTTP returns 404 outright; the wrong artifact is unreachable.

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateMetadata {
    #[cfg(not(mobile))]
    pub rid: tauri::ResourceId,
    #[cfg(mobile)]
    pub rid: u32,
    pub current_version: String,
    pub version: String,
    pub date: Option<String>,
    pub body: Option<String>,
    /// Raw manifest JSON — shared by desktop `new Update(metadata)` and the Android custom install path.
    pub raw_json: serde_json::Value,
}

/// Resolves the manifest source, then runs plugin-updater's standard check flow.
/// Channel: uses an explicitly passed `channel` (the About page always checks Stable, the
/// advanced page's Beta section checks Beta); when absent, uses the user's explicit
/// channel choice; when none chosen yet, follows the current build type.
/// Returns None = already up to date; Some(metadata) = an update is available.
#[tauri::command]
#[cfg(not(mobile))]
pub async fn app_check_update_with_channel<R: tauri::Runtime>(
    coord: CoordinatorState<'_>,
    webview: tauri::Webview<R>,
    timeout_ms: Option<u64>,
    channel: Option<UpdateChannel>,
) -> Result<Option<AppUpdateMetadata>, String> {
    use tauri_plugin_updater::UpdaterExt;

    let prefs = coord.backend().get_preferences();
    let channel = effective_update_channel(channel, &prefs, env!("CARGO_PKG_VERSION"));
    let mut builder = webview.updater_builder();
    if let Some(ms) = timeout_ms {
        builder = builder.timeout(std::time::Duration::from_millis(ms));
    }
    if matches!(channel, UpdateChannel::Beta) {
        let urls = resolve_beta_manifest_endpoints().await?;
        builder = builder
            .endpoints(urls)
            .map_err(|e| format!("set beta endpoints: {e}"))?;
    }
    let updater = builder.build().map_err(|e| format!("build updater: {e}"))?;
    let update = updater
        .check()
        .await
        .map_err(|e| format!("check update failed: {e}"))?;

    let Some(update) = update else {
        return Ok(None);
    };
    // Passing through the date field would require the time crate; the frontend
    // AutoUpdate.tsx never uses date, so set it to None here and avoid pulling a new dep
    // into src-tauri/Cargo.toml.
    let metadata = AppUpdateMetadata {
        current_version: update.current_version.clone(),
        version: update.version.clone(),
        date: None,
        body: update.body.clone(),
        raw_json: update.raw_json.clone(),
        rid: webview.resources_table().add(update),
    };
    Ok(Some(metadata))
}

/// Builds the -beta manifest URL pair from the latest prerelease tag found by
/// fetch_latest_beta_release.
/// Order: mirror first (fastgit.cc proxies GitHub), then direct — matching tauri.conf's
/// existing Stable endpoints so mainland-China access hits the CDN first.
#[cfg(not(mobile))]
async fn resolve_beta_manifest_endpoints() -> Result<Vec<url::Url>, String> {
    let Some(latest) = fetch_latest_beta_release().await? else {
        return Err("尚未发布过 Beta 版本".to_string());
    };
    let tag = latest.tag_name;
    // {{target}} / {{arch}} placeholders are substituted by the plugin at check time. A
    // Rust raw string (r#""#) needs no escaped double braces and is cleaner than format!-ing
    // the literal.
    let mirror = format!(
        "https://fastgit.cc/https://github.com/Open-Less/openless/releases/download/{tag}/latest-{{{{target}}}}-{{{{arch}}}}-beta-mirror.json"
    );
    let direct = format!(
        "https://github.com/Open-Less/openless/releases/download/{tag}/latest-{{{{target}}}}-{{{{arch}}}}-beta.json"
    );
    let mirror_url = url::Url::parse(&mirror).map_err(|e| format!("parse beta mirror url: {e}"))?;
    let direct_url = url::Url::parse(&direct).map_err(|e| format!("parse beta direct url: {e}"))?;
    Ok(vec![mirror_url, direct_url])
}

#[cfg(mobile)]
#[tauri::command]
pub async fn app_check_update_with_channel(
    coord: CoordinatorState<'_>,
    _timeout_ms: Option<u64>,
    channel: Option<UpdateChannel>,
) -> Result<Option<AppUpdateMetadata>, String> {
    #[cfg(target_os = "android")]
    {
        let prefs = coord.backend().get_preferences();
        let channel = effective_update_channel(channel, &prefs, env!("CARGO_PKG_VERSION"));
        return crate::android::updater::check_update(channel).await;
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (coord, channel);
        Err("应用内更新仅支持 Android".to_string())
    }
}

#[cfg(mobile)]
#[tauri::command]
pub async fn app_download_and_install_android_update(
    app: AppHandle,
    url: String,
    signature: String,
    version: String,
) -> Result<(), String> {
    // Security: validate the URL before downloading to prevent SSRF (e.g. intranet metadata
    // endpoints, localhost services). Only the known GitHub direct links and the fastgit
    // mirror prefix are allowed.
    const DIRECT_BASE: &str = "https://github.com/Open-Less/openless";
    const MIRROR_BASE: &str = "https://fastgit.cc/https://github.com/Open-Less/openless";
    if !url.starts_with(DIRECT_BASE) && !url.starts_with(MIRROR_BASE) {
        return Err(format!("不信任的更新 URL，拒绝下载: {url}"));
    }
    #[cfg(target_os = "android")]
    {
        return crate::android::updater::download_and_install(app, url, signature, version).await;
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (app, url, signature, version);
        Err("应用内更新仅支持 Android".to_string())
    }
}

/// Replace the single dictation binding under the existing settings transaction.
pub(crate) fn replace_dictation_hotkey(
    coord: &Coordinator,
    binding: ShortcutBinding,
) -> Result<(), String> {
    let _host_guard = coord.lock_settings_host();
    let mut prefs = coord.backend().get_preferences();
    crate::shortcut_binding::validate_binding(&binding).map_err(|error| error.to_string())?;
    reject_bare_shift_dictation_shortcut(&binding)?;
    #[cfg(target_os = "macos")]
    {
        let native = crate::macos_dictation_key::PRIMARY;
        if prefs.dictation_hotkey.primary == native || binding.primary == native {
            if coord.dictation_shortcut_is_busy() {
                return Err("macDictationKeyBusy".into());
            }
        }
    }
    if binding == prefs.dictation_hotkey {
        // Re-saving an unchanged binding must retry a failed startup listener.
        return coord.try_update_native_dictation_binding();
    }
    prefs.dictation_hotkey = binding;
    sync_dictation_hotkey_legacy_fields(&mut prefs);
    reject_hotkey_collisions(&prefs)?;
    coord
        .backend()
        .update_settings(
            prefs,
            openless_core::SettingsUpdateOptions::STRICT,
            &TauriSettingsRuntime::new(coord),
        )
        .map(|_| ())
        .map_err(settings_save_error)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSnapshot {
    pub preferences: UserPreferences,
    pub revision: u64,
}

#[tauri::command]
pub fn get_settings_snapshot(core: CoreState<'_>) -> Result<SettingsSnapshot, String> {
    settings_snapshot(&core)
}

fn settings_snapshot(backend: &openless_core::OpenLessBackend) -> Result<SettingsSnapshot, String> {
    for _ in 0..3 {
        let revision = backend.snapshot().preferences_revision;
        let preferences = backend.get_preferences();
        if backend.snapshot().preferences_revision == revision {
            return Ok(SettingsSnapshot {
                preferences,
                revision,
            });
        }
    }
    Err("settings are changing; retry the read".into())
}

fn persist_setting_fields(
    coord: &Coordinator,
    edits: &std::collections::BTreeMap<String, serde_json::Value>,
) -> Result<(), String> {
    let _host_guard = coord.lock_settings_host();
    openless_core::preference_patch::update_fields(
        edits,
        || {
            (
                coord.backend().snapshot().preferences_revision,
                coord.backend().get_preferences(),
            )
        },
        |mut prefs, revision| {
            preserve_update_channel_preferences(&mut prefs, &coord.backend().get_preferences());
            coord.backend().update_settings(
                prefs,
                openless_core::SettingsUpdateOptions::SETTINGS_DOCUMENT.at_revision(revision),
                &TauriSettingsRuntime::new(coord),
            )
        },
    )
    .map(|_| ())
    .map_err(settings_save_error)
}

#[cfg(not(mobile))]
#[tauri::command]
pub async fn update_setting_fields(
    coord: CoordinatorState<'_>,
    app: AppHandle,
    edits: std::collections::BTreeMap<String, serde_json::Value>,
) -> Result<SettingsSnapshot, String> {
    let prefs = coord.backend().get_preferences();
    set_settings(coord.clone(), app, prefs, Some(edits)).await?;
    settings_snapshot(&coord.backend())
}

#[cfg(mobile)]
#[tauri::command]
pub async fn update_setting_fields(
    coord: CoordinatorState<'_>,
    edits: std::collections::BTreeMap<String, serde_json::Value>,
) -> Result<SettingsSnapshot, String> {
    let prefs = coord.backend().get_preferences();
    set_settings(coord.clone(), prefs, Some(edits)).await?;
    settings_snapshot(&coord.backend())
}
