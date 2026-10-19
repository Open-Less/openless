//! Encrypted cloud sync card + its dialogs (Tauri `CloudSyncSection` parity).
//!
//! The card is a status mirror: everything it shows comes from
//! `CloudSyncUi::status`, and every gesture is a [`CloudSyncAction`] the host
//! turns into a Core call. Passwords live in `CloudSyncUi` only while the form is
//! open and are cleared on dispatch or close.

use openless_core::cloud_sync_e2ee::{ConflictSide, KeyState, RestoreMode, RestorePreview};
use openless_linux_egui::cloud_sync_ui::{
    CloudSyncAction, CloudSyncDialog, CloudSyncIntent, CloudSyncUi,
};
use openless_linux_egui::{fmt_l10n, tr_l10n, Lang};

use super::layout;
use super::settings::{card, row_desc, toggle_row};
use super::view_model::{FrontendAction, FrontendViewModel};
use crate::ui::theme;

fn act(action: CloudSyncAction) -> FrontendAction {
    FrontendAction::SettingsCloudSync(action)
}

/// The privacy section's cloud sync card.
pub fn cloud_sync_card(
    ui: &mut egui::Ui,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
) {
    let lang = vm.lang;
    let state = &vm.cloud_sync;
    card(ui, "", "", |ui| {
        // ── header ────────────────────────────────────────────────────────
        row_desc(
            ui,
            tr_l10n(lang, "cloudSyncE2ee.title"),
            tr_l10n(lang, "cloudSyncE2ee.description"),
            |ui| {
                if small_button(ui, tr_l10n(lang, "cloudSyncE2ee.refresh"), false) && !state.loading
                {
                    actions.push(act(CloudSyncAction::Reload));
                }
            },
        );

        // ── master toggle ────────────────────────────────────────────────
        let toggle_enabled = state.toggle_enabled();
        toggle_row(
            ui,
            tr_l10n(lang, "cloudSyncE2ee.enable"),
            "",
            state.status.as_ref().is_some_and(|status| status.enabled),
            || {
                if toggle_enabled {
                    let current = state.status.as_ref().is_some_and(|status| status.enabled);
                    actions.push(act(CloudSyncAction::Toggle(!current)));
                }
            },
        );

        if state.loading {
            note(ui, tr_l10n(lang, "cloudSyncE2ee.loading"), theme::INK_3);
        }

        // ── identity ─────────────────────────────────────────────────────
        let custom_configured = !state.saved_origin.trim().is_empty();
        if !custom_configured
            && !state.signed_in_to_sync()
            && !state.signed_in
            && small_button(ui, tr_l10n(lang, "cloudSyncE2ee.signIn"), true)
        {
            actions.push(FrontendAction::MarketplaceAuthStart);
        }
        if state.signed_in_to_sync() {
            let login = state
                .status
                .as_ref()
                .and_then(|status| status.account.as_ref())
                .map(|account| account.login.clone())
                .unwrap_or_default();
            row_desc(ui, tr_l10n(lang, "cloudSyncE2ee.account"), "", |ui| {
                ui.label(
                    egui::RichText::new(if login.is_empty() {
                        "GitHub".to_string()
                    } else {
                        format!("@{login}")
                    })
                    .size(12.0)
                    .color(theme::INK_2),
                );
                if small_button(ui, tr_l10n(lang, "cloudSyncE2ee.signOut"), false) {
                    actions.push(act(CloudSyncAction::SignOut));
                }
            });
        }

        // ── status block ─────────────────────────────────────────────────
        if let Some(status) = &state.status {
            let (title_key, detail_key) = state.focus_keys();
            ui.label(
                egui::RichText::new(tr_l10n(lang, title_key))
                    .size(12.5)
                    .strong()
                    .color(theme::INK),
            );
            if !detail_key.is_empty() {
                note(ui, tr_l10n(lang, detail_key), theme::INK_3);
            }
            let key_line = if status.key_state == KeyState::Unlocked {
                tr_l10n(lang, "cloudSyncE2ee.keyUnlocked")
            } else {
                tr_l10n(lang, "cloudSyncE2ee.keyLocked")
            };
            note(ui, key_line, theme::INK_2);
            let snapshot_line = match status.has_cloud_snapshot {
                Some(true) => tr_l10n(lang, "cloudSyncE2ee.hasSnapshot"),
                Some(false) => tr_l10n(lang, "cloudSyncE2ee.noSnapshot"),
                None => tr_l10n(lang, "cloudSyncE2ee.snapshotUnknown"),
            };
            note(ui, snapshot_line, theme::INK_2);
            if let Some(at) = &status.last_successful_sync_at {
                note(
                    ui,
                    &fmt_l10n(lang, "cloudSyncE2ee.lastSync", &[at]),
                    theme::INK_3,
                );
            }
            note(ui, tr_l10n(lang, state.state_key()), theme::INK_3);

            // ── actions ──────────────────────────────────────────────────
            ui.horizontal_wrapped(|ui| {
                let locked = status.key_state != KeyState::Unlocked;
                if locked
                    && status.has_cloud_snapshot == Some(true)
                    && button(
                        ui,
                        tr_l10n(lang, "cloudSyncE2ee.unlock"),
                        ButtonKind::Ghost,
                        !state.working(),
                    )
                {
                    actions.push(act(CloudSyncAction::Prepare(CloudSyncIntent::Unlock)));
                }
                let sync_label = if status.pending_operation_id.is_some() {
                    "cloudSyncE2ee.checkPending"
                } else {
                    "cloudSyncE2ee.syncNow"
                };
                if button(
                    ui,
                    tr_l10n(lang, sync_label),
                    ButtonKind::Blue,
                    state.can_sync(),
                ) {
                    actions.push(act(CloudSyncAction::SyncNow));
                }
                if button(
                    ui,
                    tr_l10n(lang, "cloudSyncE2ee.restore"),
                    ButtonKind::Ghost,
                    status.has_cloud_snapshot == Some(true)
                        && !state.working()
                        && !status.recovery_required,
                ) {
                    actions.push(act(CloudSyncAction::OpenDialog(CloudSyncDialog::Consent {
                        intent: CloudSyncIntent::Restore,
                    })));
                }
                if !locked {
                    if button(
                        ui,
                        tr_l10n(lang, "cloudSyncE2ee.lock"),
                        ButtonKind::Ghost,
                        !state.working(),
                    ) {
                        actions.push(act(CloudSyncAction::Lock));
                    }
                    if button(
                        ui,
                        tr_l10n(lang, "cloudSyncE2ee.changePassword"),
                        ButtonKind::Ghost,
                        !state.working() && !status.recovery_required,
                    ) {
                        actions.push(act(CloudSyncAction::OpenDialog(
                            CloudSyncDialog::ChangePassword,
                        )));
                    }
                }
                if button(
                    ui,
                    tr_l10n(lang, "cloudSyncE2ee.delete"),
                    ButtonKind::Ghost,
                    status.has_cloud_snapshot == Some(true)
                        && status.vault_id.is_some()
                        && status.remote_revision.is_some()
                        && !state.working(),
                ) {
                    if let (Some(vault_id), Some(revision)) =
                        (status.vault_id.clone(), status.remote_revision.clone())
                    {
                        actions.push(act(CloudSyncAction::OpenDialog(CloudSyncDialog::Delete {
                            vault_id,
                            revision,
                        })));
                    }
                }
            });
        }

        // ── working / notices ────────────────────────────────────────────
        if state.working() {
            ui.horizontal(|ui| {
                note(ui, tr_l10n(lang, "cloudSyncE2ee.working"), theme::INK_2);
                if let Some(task) = state
                    .status
                    .as_ref()
                    .and_then(|status| status.task_id.clone())
                {
                    if small_button(ui, tr_l10n(lang, "cloudSyncE2ee.cancelTask"), false) {
                        actions.push(act(CloudSyncAction::Cancel(task)));
                    }
                }
            });
        }
        if let Some(notice) = &state.notice {
            // Dynamic key: `tr_l10n` only takes `&'static str`.
            note(ui, &fmt_l10n(lang, notice, &[]), theme::BLUE);
        }
        // 键必须是 catalog 里真实存在的字面量：之前用 `format!` 拼键，界面上
        // 出现的是裸键（`cloudSyncE2ee.errors.service_unavailable`）。
        if let Some(error) = &state.error {
            let key = openless_linux_egui::cloud_sync_ui::error_key(error);
            note(ui, tr_l10n(lang, key), theme::ERR);
        } else if let Some(last) = state
            .status
            .as_ref()
            .and_then(|status| status.last_error.as_ref())
        {
            let key = openless_linux_egui::cloud_sync_ui::error_key(&last.code);
            note(ui, tr_l10n(lang, key), theme::ERR);
        }
        note(
            ui,
            tr_l10n(lang, "cloudSyncE2ee.passwordWarning"),
            theme::INK_3,
        );

        // ── custom server ────────────────────────────────────────────────
        row_desc(
            ui,
            tr_l10n(lang, "cloudSyncE2ee.customServerTitle"),
            tr_l10n(lang, "cloudSyncE2ee.customServerHint"),
            |ui| {
                let mut origin = state.custom_origin.clone();
                let response = layout::text_input(
                    ui,
                    &mut origin,
                    egui::Id::unique("openless-cloud-sync-origin"),
                    "https://sync.example.com",
                    ui.available_width().min(280.0),
                    false,
                );
                if response.changed() {
                    actions.push(act(CloudSyncAction::CustomOrigin(origin.clone())));
                }
            },
        );
        row_desc(
            ui,
            tr_l10n(lang, "cloudSyncE2ee.customServerToken"),
            "",
            |ui| {
                let mut token = state.custom_token.clone();
                let response = layout::text_input(
                    ui,
                    &mut token,
                    egui::Id::unique("openless-cloud-sync-token"),
                    "",
                    ui.available_width().min(280.0),
                    true,
                );
                if response.changed() {
                    actions.push(act(CloudSyncAction::CustomToken(token.clone())));
                }
                if small_button(ui, tr_l10n(lang, "cloudSyncE2ee.customServerSave"), true)
                    && !state.working()
                {
                    actions.push(act(CloudSyncAction::ConfigureServer));
                }
            },
        );
    });
}

#[derive(Clone, Copy)]
enum ButtonKind {
    Ghost,
    Blue,
}

fn button(ui: &mut egui::Ui, label: &str, kind: ButtonKind, enabled: bool) -> bool {
    let text = egui::RichText::new(label).size(11.5);
    let text = match kind {
        ButtonKind::Blue => text.color(theme::SURFACE),
        ButtonKind::Ghost => text,
    };
    let mut button = egui::Button::new(text)
        .corner_radius(egui::CornerRadius::same(8))
        .min_size(egui::vec2(0.0, 26.0));
    button = match kind {
        ButtonKind::Blue => button.fill(theme::BLUE),
        ButtonKind::Ghost => button
            .fill(theme::SURFACE_2)
            .stroke(egui::Stroke::new(0.8, theme::LINE)),
    };
    ui.add_enabled(enabled, button).clicked()
}

fn small_button(ui: &mut egui::Ui, label: &str, primary: bool) -> bool {
    button(
        ui,
        label,
        if primary {
            ButtonKind::Blue
        } else {
            ButtonKind::Ghost
        },
        true,
    )
}

fn note(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
    ui.add(
        egui::Label::new(egui::RichText::new(text).size(11.5).color(color))
            .wrap()
            .halign(egui::Align::Min),
    );
}

/// Modal shell for every cloud-sync dialog. Painted above the settings sheet so
/// the card underneath cannot be clicked while a decision is pending.
pub fn cloud_sync_overlay(
    ctx: &egui::Context,
    body: egui::Rect,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
) {
    if !vm.cloud_sync.dialog.is_open() {
        return;
    }
    let lang = vm.lang;
    let state = &vm.cloud_sync;
    egui::Area::new(egui::Id::unique("openless-cloud-sync-dialog"))
        .order(egui::Order::Foreground)
        .sense(egui::Sense::click())
        .fixed_pos(body.min)
        .show(ctx, |ui| {
            ui.set_min_size(body.size());
            ui.set_clip_rect(body);
            layout::paint_blurred_overlay(ctx, ui, body, egui::CornerRadius::ZERO);
            let width = body.width().min(460.0);
            let max_height = body.height() - 80.0;
            let panel = egui::Rect::from_center_size(
                body.center(),
                egui::vec2(width, max_height.min(560.0)),
            );
            egui::Frame::new()
                .fill(theme::SURFACE)
                .stroke(egui::Stroke::new(0.5, theme::LINE))
                .corner_radius(egui::CornerRadius::same(14))
                .inner_margin(egui::Margin::symmetric(18, 18))
                .show(
                    &mut ui.new_child(egui::UiBuilder::new().max_rect(panel)),
                    |ui| {
                        ui.set_width(panel.width() - 36.0);
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            dialog_contents(ui, lang, state, actions);
                        });
                    },
                );
        });
}

fn dialog_contents(
    ui: &mut egui::Ui,
    lang: Lang,
    state: &CloudSyncUi,
    actions: &mut Vec<FrontendAction>,
) {
    let dialog = state.dialog.clone();
    // `busy` freezes the forms: a second click must not queue another Core call.
    let busy = state.busy;
    match dialog {
        CloudSyncDialog::None => {}
        CloudSyncDialog::Consent { intent } => {
            ui.label(
                egui::RichText::new(tr_l10n(lang, "cloudSyncE2ee.consentTitle"))
                    .size(13.5)
                    .strong(),
            );
            note(
                ui,
                tr_l10n(lang, "cloudSyncE2ee.consentDescription"),
                theme::INK_2,
            );
            let mut checked = state.consent_checked;
            if ui
                .add(egui::Checkbox::new(
                    &mut checked,
                    tr_l10n(lang, "cloudSyncE2ee.consentCheck"),
                ))
                .changed()
            {
                actions.push(act(CloudSyncAction::ConsentChecked(checked)));
            }
            ui.horizontal(|ui| {
                if small_button(ui, tr_l10n(lang, "common.cancel"), false) {
                    actions.push(act(CloudSyncAction::CloseDialog));
                }
                if button(
                    ui,
                    tr_l10n(lang, "cloudSyncE2ee.continue"),
                    ButtonKind::Blue,
                    state.consent_checked && !busy,
                ) {
                    // 同意之后还要过一屏协议警告，形态与 Tauri 一致。
                    actions.push(act(CloudSyncAction::OpenDialog(
                        CloudSyncDialog::Protocol { intent },
                    )));
                }
            });
        }
        CloudSyncDialog::Protocol { intent } => {
            ui.label(
                egui::RichText::new(tr_l10n(lang, "cloudSyncE2ee.protocolTitle"))
                    .size(13.5)
                    .strong(),
            );
            for key in [
                "cloudSyncE2ee.protocolIntro",
                "cloudSyncE2ee.protocolPasswordTitle",
                "cloudSyncE2ee.protocolPassword",
                "cloudSyncE2ee.protocolEncryptionTitle",
                "cloudSyncE2ee.protocolEncryption",
                "cloudSyncE2ee.protocolExcludedTitle",
                "cloudSyncE2ee.protocolExcluded",
            ] {
                note(ui, tr_l10n(lang, key), theme::INK_2);
            }
            ui.horizontal(|ui| {
                if small_button(ui, tr_l10n(lang, "common.cancel"), false) {
                    actions.push(act(CloudSyncAction::CloseDialog));
                }
                if button(
                    ui,
                    tr_l10n(lang, "cloudSyncE2ee.continue"),
                    ButtonKind::Blue,
                    !busy,
                ) {
                    actions.push(act(CloudSyncAction::Prepare(intent)));
                }
            });
        }
        CloudSyncDialog::Create { .. } => {
            password_form(
                ui,
                lang,
                state,
                actions,
                "cloudSyncE2ee.createTitle",
                "创建",
                true,
                false,
            );
        }
        CloudSyncDialog::Unlock { intent } => {
            password_form(
                ui,
                lang,
                state,
                actions,
                "cloudSyncE2ee.unlockTitle",
                "解锁",
                false,
                false,
            );
            // `intent` 只用于解锁后的下一步，表单本身不区分。
            let _ = intent;
        }
        CloudSyncDialog::ChangePassword => {
            ui.label(
                egui::RichText::new(tr_l10n(lang, "cloudSyncE2ee.passwordTitle"))
                    .size(13.5)
                    .strong(),
            );
            current_password_row(ui, lang, state, actions);
            password_form(ui, lang, state, actions, "", "保存", true, true);
        }
        CloudSyncDialog::Restore {
            preview,
            enable_after,
        } => restore_review(ui, lang, state, actions, &preview, enable_after),
        CloudSyncDialog::Disable => {
            ui.label(
                egui::RichText::new(tr_l10n(lang, "cloudSyncE2ee.disableTitle"))
                    .size(13.5)
                    .strong(),
            );
            note(
                ui,
                tr_l10n(lang, "cloudSyncE2ee.disableDescription"),
                theme::INK_2,
            );
            ui.horizontal(|ui| {
                if small_button(ui, tr_l10n(lang, "common.cancel"), false) {
                    actions.push(act(CloudSyncAction::CloseDialog));
                }
                if button(
                    ui,
                    tr_l10n(lang, "cloudSyncE2ee.confirmDisable"),
                    ButtonKind::Blue,
                    !busy,
                ) {
                    actions.push(act(CloudSyncAction::SetEnabled(false)));
                }
            });
        }
        CloudSyncDialog::Delete { .. } => {
            ui.label(
                egui::RichText::new(tr_l10n(lang, "cloudSyncE2ee.deleteTitle"))
                    .size(13.5)
                    .strong(),
            );
            note(
                ui,
                tr_l10n(lang, "cloudSyncE2ee.deleteDescription"),
                theme::INK_2,
            );
            if let Some(days) = state
                .status
                .as_ref()
                .and_then(|status| status.backup_retention_days)
            {
                note(
                    ui,
                    &fmt_l10n(lang, "cloudSyncE2ee.backupRetention", &[&days]),
                    theme::INK_3,
                );
            }
            ui.horizontal(|ui| {
                if small_button(ui, tr_l10n(lang, "common.cancel"), false) {
                    actions.push(act(CloudSyncAction::CloseDialog));
                }
                if button(
                    ui,
                    tr_l10n(lang, "cloudSyncE2ee.confirmDelete"),
                    ButtonKind::Blue,
                    !busy,
                ) {
                    actions.push(act(CloudSyncAction::DeleteRemote));
                }
            });
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn password_form(
    ui: &mut egui::Ui,
    lang: Lang,
    state: &CloudSyncUi,
    actions: &mut Vec<FrontendAction>,
    title_key: &str,
    submit: &str,
    with_confirmation: bool,
    _changing: bool,
) {
    if !title_key.is_empty() {
        ui.label(
            egui::RichText::new(fmt_l10n(lang, title_key, &[]))
                .size(13.5)
                .strong(),
        );
    }
    let busy = state.busy;
    let intent = state.intent.unwrap_or(CloudSyncIntent::Unlock);

    let mut password = state.password.clone();
    if layout::text_input(
        ui,
        &mut password,
        egui::Id::unique("openless-cloud-sync-password"),
        tr_l10n(lang, "cloudSyncE2ee.passwordPolicy"),
        ui.available_width().min(320.0),
        true,
    )
    .changed()
    {
        actions.push(act(CloudSyncAction::Password(password.clone())));
    }

    let mut confirmation = state.confirmation.clone();
    if with_confirmation
        && layout::text_input(
            ui,
            &mut confirmation,
            egui::Id::unique("openless-cloud-sync-confirmation"),
            tr_l10n(lang, "cloudSyncE2ee.confirmPassword"),
            ui.available_width().min(320.0),
            true,
        )
        .changed()
    {
        actions.push(act(CloudSyncAction::Confirmation(confirmation.clone())));
    }

    let mut remember = state.remember_key;
    if ui
        .add(egui::Checkbox::new(
            &mut remember,
            tr_l10n(lang, "cloudSyncE2ee.rememberKey"),
        ))
        .changed()
    {
        actions.push(act(CloudSyncAction::RememberKey(remember)));
    }

    let ready =
        !state.password.is_empty() && (!with_confirmation || state.password == state.confirmation);
    ui.horizontal(|ui| {
        if small_button(ui, tr_l10n(lang, "common.cancel"), false) {
            actions.push(act(CloudSyncAction::CloseDialog));
        }
        if button(ui, submit, ButtonKind::Blue, ready && !busy) {
            actions.push(act(CloudSyncAction::SubmitPassword(intent)));
        }
    });
}

fn current_password_row(
    ui: &mut egui::Ui,
    lang: Lang,
    state: &CloudSyncUi,
    actions: &mut Vec<FrontendAction>,
) {
    let mut current = state.current_password.clone();
    if layout::text_input(
        ui,
        &mut current,
        egui::Id::unique("openless-cloud-sync-current-password"),
        tr_l10n(lang, "cloudSyncE2ee.currentPassword"),
        ui.available_width().min(320.0),
        true,
    )
    .changed()
    {
        actions.push(act(CloudSyncAction::CurrentPassword(current.clone())));
    }
}

fn restore_review(
    ui: &mut egui::Ui,
    lang: Lang,
    state: &CloudSyncUi,
    actions: &mut Vec<FrontendAction>,
    preview: &RestorePreview,
    enable_after: bool,
) {
    ui.label(
        egui::RichText::new(tr_l10n(lang, "cloudSyncE2ee.restoreTitle"))
            .size(13.5)
            .strong(),
    );
    note(
        ui,
        tr_l10n(lang, "cloudSyncE2ee.restoreDescription"),
        theme::INK_2,
    );
    for (kind, count) in &preview.counts {
        note(ui, &format!("{kind}: {count}"), theme::INK_2);
    }
    if !preview.device_settings_to_review.is_empty() {
        note(
            ui,
            &fmt_l10n(
                lang,
                "cloudSyncE2ee.deviceReview",
                &[&preview.device_settings_to_review.len()],
            ),
            theme::INK_3,
        );
        note(
            ui,
            &preview.device_settings_to_review.join(", "),
            theme::INK_3,
        );
    }

    // 冲突必须先逐条选定一侧，否则 Core 会拒绝应用。
    if !preview.conflicts.is_empty() {
        note(
            ui,
            &fmt_l10n(
                lang,
                "cloudSyncE2ee.conflictProgress",
                &[
                    &state
                        .conflicts
                        .iter()
                        .filter(|(_, side)| side.is_some())
                        .count(),
                    &preview.conflicts.len(),
                ],
            ),
            theme::INK,
        );
        for item in &preview.conflicts {
            let current = state
                .conflicts
                .iter()
                .find(|(id, _)| id == &item.id)
                .and_then(|(_, side)| *side);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&item.id).size(11.5));
                for (label_key, side) in [
                    ("cloudSyncE2ee.conflictLocal", ConflictSide::Local),
                    ("cloudSyncE2ee.conflictCloud", ConflictSide::Cloud),
                ] {
                    let selected = current == Some(side);
                    let label = tr_l10n(lang, label_key);
                    let kind = if selected {
                        ButtonKind::Blue
                    } else {
                        ButtonKind::Ghost
                    };
                    if button(ui, label, kind, true) {
                        actions.push(act(CloudSyncAction::ResolveConflict(
                            item.id.clone(),
                            Some(side),
                        )));
                    }
                }
            });
        }
    }

    let ready = !state.busy && state.conflicts_resolved();
    ui.horizontal(|ui| {
        if small_button(ui, tr_l10n(lang, "common.cancel"), false) {
            actions.push(act(CloudSyncAction::CloseDialog));
        }
        if button(
            ui,
            tr_l10n(lang, "cloudSyncE2ee.replace"),
            ButtonKind::Ghost,
            ready,
        ) {
            actions.push(act(CloudSyncAction::ApplyRestore(RestoreMode::Replace)));
        }
        if button(
            ui,
            tr_l10n(lang, "cloudSyncE2ee.merge"),
            ButtonKind::Blue,
            ready,
        ) {
            actions.push(act(CloudSyncAction::ApplyRestore(RestoreMode::Merge)));
        }
    });
    if enable_after {
        note(
            ui,
            tr_l10n(lang, "cloudSyncE2ee.restoreCheck"),
            theme::INK_3,
        );
    }
}
