//! P5 host enrichment. The native implementation borrows only P4's frozen
//! original objects; model-facing data contains no native identity.

#[cfg(target_os = "macos")]
pub(crate) mod macos;
#[cfg(target_os = "macos")]
pub(crate) mod observation;

#[cfg(target_os = "macos")]
pub(crate) fn read_frozen(
    target: &crate::selection::SelectionInsertionTarget,
    grant: openless_core::browser_scene::SceneAuthorization,
    current: impl Fn() -> bool,
) -> Option<macos::Enrichment> {
    inspect(target, grant, true, current)
}

/// Called only with the existing cursor-context opt-in. It does not read a URL
/// or share any page text, and cannot grant Ask browser-context access.
#[cfg(target_os = "macos")]
pub(crate) fn input_kind(
    target: &crate::selection::SelectionInsertionTarget,
) -> Option<openless_core::browser_scene::BrowserInputKind> {
    let grant = openless_core::browser_scene::SceneAuthorization {
        app_metadata: true,
        browser_metadata: false,
        selected_text: false,
        epoch: 0,
    };
    inspect(target, grant, false, || true).map(|scene| scene.input_kind)
}

#[cfg(target_os = "macos")]
fn inspect(
    target: &crate::selection::SelectionInsertionTarget,
    grant: openless_core::browser_scene::SceneAuthorization,
    include_location: bool,
    current: impl Fn() -> bool,
) -> Option<macos::Enrichment> {
    if !grant.app_metadata || (include_location && !grant.browser_metadata) || !current() {
        return None;
    }
    let app = crate::selection::input_app_for_target(target)?;
    let browser = openless_core::input_scene::recognize(&app)?.browser?;
    crate::host_document::macos::with_frozen_target_read(target, |lease| {
        let (control, window) = unsafe { lease.ax_objects() };
        // No wrapping under get-rule: the two P4 objects are borrowed, never
        // retained or saved by P5. The synchronous read cannot cross await.
        unsafe {
            macos::enrich(
                lease.process_id(),
                browser,
                window,
                control,
                grant,
                include_location,
                || current() && lease.is_current(),
            )
        }
        .ok_or_else(|| "browserSceneUnavailable".to_string())
    })
    .ok()
    .filter(|_| current())
}
