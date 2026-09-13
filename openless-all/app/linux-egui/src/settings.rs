use std::sync::Arc;

use openless_core::shared_types::{HotkeyTrigger, ShortcutBinding};
use openless_core::{
    legacy_modifier_trigger, BackendError, BackendErrorCode, HotkeyRuntimeTarget, ProviderSlot,
    SettingsEffectFailure, SettingsEffectKind, SettingsEffectPlan, SettingsEffectReceipt,
    SettingsRuntime,
};

use crate::LinuxCredentialStore;

/// Executes Linux-only settings effects from an explicit Core target.
///
/// Implementations must not read or write `UserPreferences`. This narrow seam
/// lets contract tests replace DBus/keyring without involving an egui window.
pub trait LinuxSettingsEffects: Send + Sync {
    fn apply_hotkeys(&self, target: &HotkeyRuntimeTarget) -> Result<(), BackendError>;

    fn set_active_asr_provider(&self, provider_id: &str) -> Result<(), BackendError>;
}

/// Linux implementation of the shared settings transaction runtime.
pub struct LinuxSettingsRuntime {
    effects: Arc<dyn LinuxSettingsEffects>,
}

impl LinuxSettingsRuntime {
    /// Build the production fcitx5 + Linux credential-metadata adapter.
    pub fn new(credentials: LinuxCredentialStore) -> Self {
        Self::with_effects(Arc::new(Fcitx5SettingsEffects {
            credentials: Some(credentials),
        }))
    }

    /// Build the production fcitx5 adapter without active-provider storage.
    ///
    /// This is used only when a host injects a custom `CredentialStore` without
    /// also injecting a matching `SettingsRuntime`. Active-provider changes then
    /// fail explicitly with `Unsupported` instead of silently diverging.
    pub fn hotkeys_only() -> Self {
        Self::with_effects(Arc::new(Fcitx5SettingsEffects { credentials: None }))
    }

    pub fn with_effects(effects: Arc<dyn LinuxSettingsEffects>) -> Self {
        Self { effects }
    }

    fn reject_unsupported_hotkey_changes(plan: &SettingsEffectPlan) -> Result<(), BackendError> {
        let Some(change) = &plan.hotkeys else {
            return Ok(());
        };
        let previous = &change.previous;
        let next = &change.next;
        let unsupported = [
            (
                previous.switch_style != next.switch_style,
                "switch-style hotkey",
            ),
            (previous.open_app != next.open_app, "open-app hotkey"),
            (
                previous.style_packs != next.style_packs,
                "style-pack hotkeys",
            ),
        ];
        let names = unsupported
            .into_iter()
            .filter_map(|(changed, name)| changed.then_some(name))
            .collect::<Vec<_>>();
        if names.is_empty() {
            Ok(())
        } else {
            Err(BackendError::new(
                BackendErrorCode::Unsupported,
                format!(
                    "Linux fcitx5 settings adapter does not support changing {}",
                    names.join(", ")
                ),
            ))
        }
    }
}

impl SettingsRuntime for LinuxSettingsRuntime {
    fn prepare(
        &self,
        plan: &SettingsEffectPlan,
    ) -> Result<SettingsEffectReceipt, SettingsEffectFailure> {
        if plan.windows_keyboard.is_some() {
            return Err(SettingsEffectFailure::before_side_effect(
                BackendError::new(
                    BackendErrorCode::Unsupported,
                    "Windows keyboard settings are unavailable on the Linux host",
                ),
            ));
        }

        let mut receipt = SettingsEffectReceipt::default();
        if let Some(change) = &plan.active_asr_provider {
            if let Err(error) = self.effects.set_active_asr_provider(&change.next) {
                return Err(SettingsEffectFailure::after_side_effect(error, receipt));
            }
            receipt.applied.push(SettingsEffectKind::ActiveAsrProvider);
        }
        Ok(receipt)
    }

    fn commit(
        &self,
        plan: &SettingsEffectPlan,
        receipt: &mut SettingsEffectReceipt,
    ) -> Result<(), SettingsEffectFailure> {
        Self::reject_unsupported_hotkey_changes(plan)
            .map_err(SettingsEffectFailure::before_side_effect)?;
        let Some(change) = &plan.hotkeys else {
            return Ok(());
        };
        if !receipt.applied.contains(&SettingsEffectKind::Hotkeys) {
            receipt.applied.push(SettingsEffectKind::Hotkeys);
        }
        self.effects
            .apply_hotkeys(&change.next)
            .map_err(|error| SettingsEffectFailure::after_side_effect(error, receipt.clone()))
    }

    fn restore(
        &self,
        plan: &SettingsEffectPlan,
        receipt: &SettingsEffectReceipt,
    ) -> Result<(), BackendError> {
        let mut failures = Vec::new();
        for effect in receipt.applied.iter().rev() {
            let result = match effect {
                SettingsEffectKind::Hotkeys => plan
                    .hotkeys
                    .as_ref()
                    .map(|change| self.effects.apply_hotkeys(&change.previous))
                    .unwrap_or(Ok(())),
                SettingsEffectKind::ActiveAsrProvider => plan
                    .active_asr_provider
                    .as_ref()
                    .map(|change| self.effects.set_active_asr_provider(&change.previous))
                    .unwrap_or(Ok(())),
                SettingsEffectKind::WindowsKeyboard => Ok(()),
            };
            if let Err(error) = result {
                failures.push(error.message);
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(BackendError::new(
                BackendErrorCode::Platform,
                format!(
                    "failed to restore Linux settings effects: {}",
                    failures.join("; ")
                ),
            ))
        }
    }
}

struct Fcitx5SettingsEffects {
    credentials: Option<LinuxCredentialStore>,
}

impl LinuxSettingsEffects for Fcitx5SettingsEffects {
    fn apply_hotkeys(&self, target: &HotkeyRuntimeTarget) -> Result<(), BackendError> {
        apply_dictation_hotkey(&target.dictation)?;
        apply_action_hotkey("SetQaHotkeyRaw", target.qa.as_ref())?;
        apply_action_hotkey(
            "SetSelectionPolishHotkeyRaw",
            target.selection_polish.as_ref(),
        )?;
        apply_action_hotkey("SetTranslationHotkeyRaw", Some(&target.translation))?;
        let (symbol, states) = target
            .coding_agent_voice
            .as_ref()
            // The configured binding survives a disabled feature, but the
            // native hook must be removed until the user enables it again.
            .filter(|_| target.coding_agent_enabled)
            .map(shortcut_to_raw)
            .transpose()?
            .unwrap_or((0, 0));
        crate::fcitx5::set_less_computer_hotkey_raw(symbol, states)
    }

    fn set_active_asr_provider(&self, provider_id: &str) -> Result<(), BackendError> {
        let Some(credentials) = &self.credentials else {
            return Err(BackendError::new(
                BackendErrorCode::Unsupported,
                "the injected Linux credential store does not expose active-provider settings effects",
            ));
        };
        credentials.set_active_provider_immediate(ProviderSlot::Asr, provider_id)
    }
}

fn apply_dictation_hotkey(binding: &ShortcutBinding) -> Result<(), BackendError> {
    if let Some(trigger) = legacy_modifier_trigger(binding) {
        let symbol = modifier_trigger_keysym(trigger)?;
        return crate::fcitx5::set_raw_hotkey("SetHotkeyRaw", symbol, 0);
    }
    crate::fcitx5::set_custom_dictation_trigger(&binding_to_fcitx_key(binding))
}

fn apply_action_hotkey(
    method: &str,
    binding: Option<&ShortcutBinding>,
) -> Result<(), BackendError> {
    let (symbol, states) = binding.map(shortcut_to_raw).transpose()?.unwrap_or((0, 0));
    crate::fcitx5::set_raw_hotkey(method, symbol, states)
}

fn binding_to_fcitx_key(binding: &ShortcutBinding) -> String {
    let mut parts = Vec::new();
    for modifier in &binding.modifiers {
        let normalized = match modifier.trim().to_ascii_lowercase().as_str() {
            "ctrl" | "control" => "Control".to_string(),
            "alt" | "option" | "opt" => "Alt".to_string(),
            "shift" => "Shift".to_string(),
            "cmd" | "command" | "super" | "meta" | "win" => "Super".to_string(),
            other => other.to_string(),
        };
        if !parts.contains(&normalized) {
            parts.push(normalized);
        }
    }
    parts.push(normalize_fcitx_primary(&binding.primary));
    parts.join("+")
}

fn normalize_fcitx_primary(primary: &str) -> String {
    let trimmed = primary.trim();
    if let Some(stripped) = trimmed.strip_prefix("Key") {
        stripped.to_ascii_lowercase()
    } else {
        trimmed.to_ascii_lowercase()
    }
}

fn shortcut_to_raw(binding: &ShortcutBinding) -> Result<(u32, u32), BackendError> {
    if let Some(trigger) = legacy_modifier_trigger(binding) {
        return Ok((modifier_trigger_keysym(trigger)?, 0));
    }
    if binding.modifiers.is_empty() && binding.primary.eq_ignore_ascii_case("shift") {
        return Ok((0xffe1, 0));
    }

    let mut states = 0_u32;
    for modifier in &binding.modifiers {
        states |= match modifier.trim().to_ascii_lowercase().as_str() {
            "shift" => 1,
            "ctrl" | "control" => 4,
            "alt" | "option" | "opt" => 8,
            "cmd" | "command" | "super" | "meta" | "win" => 64,
            other => {
                return Err(BackendError::new(
                    BackendErrorCode::Unsupported,
                    format!("fcitx5 does not support modifier {other}"),
                ));
            }
        };
    }
    let (symbol, implied_shift) = primary_keysym(&binding.primary)?;
    if implied_shift {
        states |= 1;
    }
    Ok((symbol, states))
}

fn modifier_trigger_keysym(trigger: HotkeyTrigger) -> Result<u32, BackendError> {
    match trigger {
        HotkeyTrigger::RightControl | HotkeyTrigger::Fn => Ok(0xffe4),
        HotkeyTrigger::LeftControl => Ok(0xffe3),
        HotkeyTrigger::RightOption | HotkeyTrigger::RightAlt => Ok(0xffea),
        HotkeyTrigger::LeftOption => Ok(0xffe9),
        HotkeyTrigger::RightCommand => Ok(0xffec),
        HotkeyTrigger::LeftCommand => Ok(0xffeb),
        HotkeyTrigger::LeftShift => Ok(0xffe1),
        HotkeyTrigger::RightShift => Ok(0xffe2),
        HotkeyTrigger::MediaPlayPause | HotkeyTrigger::Custom => Err(BackendError::new(
            BackendErrorCode::Unsupported,
            "the selected modifier trigger is unavailable through fcitx5",
        )),
    }
}

fn primary_keysym(primary: &str) -> Result<(u32, bool), BackendError> {
    let trimmed = primary.trim();
    if trimmed.chars().count() == 1 {
        let character = trimmed.chars().next().expect("single character");
        let shifted = match character {
            ':' => Some(';'),
            '<' => Some(','),
            '>' => Some('.'),
            '?' => Some('/'),
            '|' => Some('\\'),
            '{' => Some('['),
            '}' => Some(']'),
            '"' => Some('\''),
            '~' => Some('`'),
            '_' => Some('-'),
            '+' => Some('='),
            '!' => Some('1'),
            '@' => Some('2'),
            '#' => Some('3'),
            '$' => Some('4'),
            '%' => Some('5'),
            '^' => Some('6'),
            '&' => Some('7'),
            '*' => Some('8'),
            '(' => Some('9'),
            ')' => Some('0'),
            _ => None,
        };
        let normalized = shifted.unwrap_or(character).to_ascii_lowercase();
        return Ok((normalized as u32, shifted.is_some()));
    }

    let upper = trimmed.to_ascii_uppercase();
    let symbol = match upper.as_str() {
        "ENTER" | "RETURN" => 0xff0d,
        "TAB" => 0xff09,
        "ESC" | "ESCAPE" => 0xff1b,
        "SPACE" => 0x20,
        "BACKSPACE" => 0xff08,
        "DELETE" | "DEL" => 0xffff,
        "HOME" => 0xff50,
        "END" => 0xff57,
        "PAGEUP" => 0xff55,
        "PAGEDOWN" => 0xff56,
        "ARROWUP" | "UP" => 0xff52,
        "ARROWDOWN" | "DOWN" => 0xff54,
        "ARROWLEFT" | "LEFT" => 0xff51,
        "ARROWRIGHT" | "RIGHT" => 0xff53,
        value if value.starts_with('F') => value
            .strip_prefix('F')
            .and_then(|number| number.parse::<u32>().ok())
            .filter(|number| (1..=12).contains(number))
            .map(|number| 0xffbd + number)
            .ok_or_else(|| unsupported_primary(trimmed))?,
        _ => return Err(unsupported_primary(trimmed)),
    };
    Ok((symbol, false))
}

fn unsupported_primary(primary: &str) -> BackendError {
    BackendError::new(
        BackendErrorCode::Unsupported,
        format!("fcitx5 does not support shortcut primary {primary}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_shortcut_conversion_covers_default_linux_actions() {
        let qa = ShortcutBinding {
            primary: ";".into(),
            modifiers: vec!["ctrl".into(), "shift".into()],
        };
        assert_eq!(shortcut_to_raw(&qa).unwrap(), (b';' as u32, 5));
        assert_eq!(
            shortcut_to_raw(&ShortcutBinding {
                primary: "Shift".into(),
                modifiers: Vec::new(),
            })
            .unwrap(),
            (0xffe1, 0)
        );
    }

    #[test]
    fn shifted_printable_uses_base_keysym_and_shift_state() {
        let shortcut = ShortcutBinding {
            primary: "?".into(),
            modifiers: vec!["ctrl".into()],
        };
        assert_eq!(shortcut_to_raw(&shortcut).unwrap(), (b'/' as u32, 5));
    }
}
