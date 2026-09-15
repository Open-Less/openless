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

    fn set_launch_at_login(&self, _enabled: bool) -> Result<(), BackendError> {
        Err(BackendError::new(
            BackendErrorCode::Unsupported,
            "launch-at-login is unavailable",
        ))
    }
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
            autostart: production_autostart(),
        }))
    }

    /// Build the production fcitx5 adapter without active-provider storage.
    ///
    /// This is used only when a host injects a custom `CredentialStore` without
    /// also injecting a matching `SettingsRuntime`. Active-provider changes then
    /// fail explicitly with `Unsupported` instead of silently diverging.
    pub fn hotkeys_only() -> Self {
        Self::with_effects(Arc::new(Fcitx5SettingsEffects {
            credentials: None,
            autostart: production_autostart(),
        }))
    }

    pub fn with_effects(effects: Arc<dyn LinuxSettingsEffects>) -> Self {
        Self { effects }
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
        if let Some(change) = &plan.launch_at_login {
            if let Err(error) = self.effects.set_launch_at_login(change.next) {
                return Err(SettingsEffectFailure::after_side_effect(error, receipt));
            }
            receipt.applied.push(SettingsEffectKind::LaunchAtLogin);
        }
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
                SettingsEffectKind::LaunchAtLogin => plan
                    .launch_at_login
                    .as_ref()
                    .map(|change| self.effects.set_launch_at_login(change.previous))
                    .unwrap_or(Ok(())),
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
    autostart: Result<crate::AutostartManager, String>,
}

fn production_autostart() -> Result<crate::AutostartManager, String> {
    std::env::current_exe()
        .map_err(|error| format!("resolve current executable for autostart: {error}"))
        .and_then(|executable| {
            crate::AutostartManager::detect(executable)
                .map_err(|error| format!("initialize XDG autostart manager: {error}"))
        })
}

impl LinuxSettingsEffects for Fcitx5SettingsEffects {
    fn apply_hotkeys(&self, target: &HotkeyRuntimeTarget) -> Result<(), BackendError> {
        // 一行摘要，用户可据此确认「到底注册了哪些键」以及哪些被拒绝。
        log::info!(
            "[fcitx] hotkey registration: {}",
            registration_summary(target)
        );
        apply_dictation_hotkey(&target.dictation)?;
        apply_action_hotkey("SetQaHotkeyRaw", target.qa.as_ref())?;
        apply_action_hotkey(
            "SetSelectionPolishHotkeyRaw",
            target.selection_polish.as_ref(),
        )?;
        apply_action_hotkey("SetTranslationHotkeyRaw", Some(&target.translation))?;
        tolerate_optional_fcitx_method(apply_action_hotkey(
            "SetSwitchStyleHotkeyRaw",
            target.switch_style.as_ref(),
        ))?;
        tolerate_optional_fcitx_method(apply_action_hotkey(
            "SetOpenAppHotkeyRaw",
            target.open_app.as_ref(),
        ))?;
        let mut style_pack_hotkeys = Vec::with_capacity(target.style_packs.len());
        for hotkey in &target.style_packs {
            let (symbol, states) = registerable_raw(&hotkey.binding)?;
            // symbol 0 = 被判为不可注册（裸修饰键/空键），直接不报给插件。
            if symbol == 0 {
                continue;
            }
            style_pack_hotkeys.push((hotkey.pack_id.clone(), symbol, states));
        }
        tolerate_optional_fcitx_method(crate::fcitx5::set_style_pack_hotkeys(style_pack_hotkeys))?;
        let (symbol, states) = target
            .coding_agent_voice
            .as_ref()
            // The configured binding survives a disabled feature, but the
            // native hook must be removed until the user enables it again.
            .filter(|_| target.coding_agent_enabled)
            .map(registerable_raw)
            .transpose()?
            .unwrap_or((0, 0));
        tolerate_optional_fcitx_method(crate::fcitx5::set_less_computer_hotkey_raw(symbol, states))
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

    fn set_launch_at_login(&self, enabled: bool) -> Result<(), BackendError> {
        let manager = self
            .autostart
            .as_ref()
            .map_err(|message| BackendError::new(BackendErrorCode::Platform, message.clone()))?;
        manager.set_enabled(enabled).map_err(|error| {
            BackendError::new(
                BackendErrorCode::Platform,
                format!("update XDG launch-at-login entry: {error}"),
            )
        })
    }
}

fn tolerate_optional_fcitx_method(result: Result<(), BackendError>) -> Result<(), BackendError> {
    match result {
        Err(error)
            if error.message.contains("Unknown method")
                || error.message.contains("UnknownMethod") =>
        {
            log::warn!(
                "[fcitx] running addon lacks an optional extended hotkey method; continuing with the legacy interface: {}",
                error.message
            );
            Ok(())
        }
        result => result,
    }
}

fn apply_dictation_hotkey(binding: &ShortcutBinding) -> Result<(), BackendError> {
    if is_bare_modifier_binding(binding) {
        warn_bare_modifier("dictation", &binding.primary);
        // 清掉可能存在的旧注册：脏数据不能继续吞键。
        return crate::fcitx5::set_raw_hotkey("SetHotkeyRaw", 0, 0);
    }
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
    let raw = binding.map(registerable_raw).transpose()?.unwrap_or((0, 0));
    crate::fcitx5::set_raw_hotkey(method, raw.0, raw.1)
}

/// Convert a binding into the `(keysym, states)` pair fcitx5 should grab, or
/// `(0, 0)` when the binding must not be registered at all.
///
/// A modifier-only binding (`Shift`, `LeftControl`, …) is the dangerous case:
/// fcitx5's addon matches `sym == registered && states == registered` and calls
/// `filterAndAccept()`, so the modifier never reaches any application — typing
/// uppercase (Shift+letter) or any Ctrl/Alt combination stops working system
/// wide. Those bindings are refused instead of registered.
pub(crate) fn registerable_raw(binding: &ShortcutBinding) -> Result<(u32, u32), BackendError> {
    if is_bare_modifier_binding(binding) {
        warn_bare_modifier("hotkey", &binding.primary);
        return Ok((0, 0));
    }
    shortcut_to_raw(binding)
}

/// True for `primary` = a bare modifier with no other modifier held.
pub fn is_bare_modifier_binding(binding: &ShortcutBinding) -> bool {
    if !binding.modifiers.is_empty() {
        return false;
    }
    if legacy_modifier_trigger(binding).is_some() {
        return true;
    }
    matches!(
        binding.primary.trim().to_ascii_lowercase().as_str(),
        "shift"
            | "control"
            | "ctrl"
            | "alt"
            | "option"
            | "opt"
            | "super"
            | "meta"
            | "win"
            | "cmd"
            | "command"
            | "fn"
            | "function"
            | "mediaplaypause"
            | "mediaplay"
            | "playpause"
    )
}

fn warn_bare_modifier(field: &str, primary: &str) {
    log::warn!(
        "[fcitx] refusing to register the modifier-only {field} hotkey '{primary}': a bare modifier grab would swallow that key for every application; pick a real key combination instead"
    );
}

/// One-line summary of what the next [`apply_hotkeys`] will register.
pub(crate) fn registration_summary(target: &HotkeyRuntimeTarget) -> String {
    fn describe(binding: Option<&ShortcutBinding>) -> String {
        match binding {
            None => "-".to_string(),
            Some(binding) if is_bare_modifier_binding(binding) => {
                format!("refused(modifier-only {})", binding.primary)
            }
            Some(binding) => {
                let mut parts: Vec<String> = binding
                    .modifiers
                    .iter()
                    .map(|modifier| normalize_modifier_tag(modifier))
                    .filter(|tag| !tag.is_empty())
                    .collect();
                parts.push(binding.primary.clone());
                parts.join("+")
            }
        }
    }
    format!(
        "dictation={} qa={} selection_polish={} translation={} switch_style={} open_app={} style_packs={} less_computer={}",
        describe(Some(&target.dictation)),
        describe(target.qa.as_ref()),
        describe(target.selection_polish.as_ref()),
        describe(Some(&target.translation)),
        describe(target.switch_style.as_ref()),
        describe(target.open_app.as_ref()),
        target.style_packs.len(),
        if target.coding_agent_enabled {
            describe(target.coding_agent_voice.as_ref())
        } else {
            "disabled".to_string()
        },
    )
}

/// Canonical lowercase tag for a modifier; unknown tags become empty so the
/// caller can drop them instead of producing a meaningless combination.
pub(crate) fn normalize_modifier_tag(modifier: &str) -> String {
    match modifier.trim().to_ascii_lowercase().as_str() {
        "ctrl" | "control" => "ctrl".to_string(),
        "alt" | "option" | "opt" => "alt".to_string(),
        "shift" => "shift".to_string(),
        "cmd" | "command" | "super" | "meta" | "win" => "super".to_string(),
        other => {
            log::warn!("[fcitx] dropping unknown hotkey modifier '{other}'");
            String::new()
        }
    }
}

fn binding_to_fcitx_key(binding: &ShortcutBinding) -> String {
    let mut parts = Vec::new();
    for modifier in &binding.modifiers {
        let normalized = match normalize_modifier_tag(modifier).as_str() {
            "ctrl" => "Control".to_string(),
            "alt" => "Alt".to_string(),
            "shift" => "Shift".to_string(),
            "super" => "Super".to_string(),
            _ => continue,
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

pub(crate) fn shortcut_to_raw(binding: &ShortcutBinding) -> Result<(u32, u32), BackendError> {
    if let Some(trigger) = legacy_modifier_trigger(binding) {
        return Ok((modifier_trigger_keysym(trigger)?, 0));
    }
    if binding.modifiers.is_empty() && binding.primary.eq_ignore_ascii_case("shift") {
        return Ok((0xffe1, 0));
    }

    let mut states = 0_u32;
    for modifier in &binding.modifiers {
        // 未知修饰键只丢弃并告警：若直接返回 Err，整轮 apply_hotkeys 会中止，
        // 后面的热键（含 QA）就全都注册不上——一个脏修饰键不该有这个后果。
        states |= match normalize_modifier_tag(modifier).as_str() {
            "shift" => 1,
            "ctrl" => 4,
            "alt" => 8,
            "super" => 64,
            _ => 0,
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
    fn modifier_only_bindings_are_never_registered() {
        // 这些是用户机器上真实出现过的脏数据/默认值：注册进去会让插件吞掉
        // 该修饰键（Shift+字母打不出大写、Ctrl 组合失效）。
        for primary in [
            "LeftControl",
            "RightControl",
            "LeftShift",
            "Shift",
            "Alt",
            "Super",
            "LeftCommand",
        ] {
            let binding = ShortcutBinding {
                primary: primary.into(),
                modifiers: Vec::new(),
            };
            assert!(
                is_bare_modifier_binding(&binding),
                "{primary} must be recognised as modifier-only"
            );
            assert_eq!(registerable_raw(&binding).unwrap(), (0, 0), "{primary}");
        }
        // 真实组合键照常注册。
        let qa = ShortcutBinding {
            primary: ":".into(),
            modifiers: vec!["ctrl".into(), "shift".into()],
        };
        assert!(!is_bare_modifier_binding(&qa));
        assert_eq!(registerable_raw(&qa).unwrap(), (b';' as u32, 5));
        // 有修饰位时 primary 是字母，绝不算裸修饰键。
        let dictation = ShortcutBinding {
            primary: "A".into(),
            modifiers: vec!["alt".into()],
        };
        assert!(!is_bare_modifier_binding(&dictation));
    }

    #[test]
    fn unknown_modifiers_are_dropped_instead_of_aborting() {
        // "cmd"/"Command" 是 macOS 写法；"banana" 是彻底未知的脏值。
        let binding = ShortcutBinding {
            primary: "Enter".into(),
            modifiers: vec!["cmd".into(), "shift".into(), "banana".into()],
        };
        assert_eq!(shortcut_to_raw(&binding).unwrap(), (0xff0d, 64 | 1));
        assert_eq!(binding_to_fcitx_key(&binding), "Super+Shift+enter");
    }

    #[test]
    fn registration_summary_flags_refused_bindings() {
        let target = HotkeyRuntimeTarget {
            dictation: ShortcutBinding {
                primary: "A".into(),
                modifiers: vec!["alt".into()],
            },
            dictation_mode: openless_core::shared_types::HotkeyMode::Toggle,
            qa: Some(ShortcutBinding {
                primary: ":".into(),
                modifiers: vec!["ctrl".into(), "shift".into()],
            }),
            translation: ShortcutBinding {
                primary: "Z".into(),
                modifiers: vec!["alt".into()],
            },
            switch_style: None,
            open_app: None,
            selection_polish: Some(ShortcutBinding {
                primary: "X".into(),
                modifiers: vec!["alt".into()],
            }),
            coding_agent_enabled: true,
            coding_agent_voice: Some(ShortcutBinding {
                primary: "LeftControl".into(),
                modifiers: Vec::new(),
            }),
            style_packs: Vec::new(),
        };
        let summary = registration_summary(&target);
        assert!(summary.contains("dictation=alt+A"), "{summary}");
        assert!(summary.contains("qa=ctrl+shift+:"), "{summary}");
        assert!(
            summary.contains("less_computer=refused(modifier-only LeftControl)"),
            "{summary}"
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

    #[test]
    fn legacy_addon_may_omit_optional_extended_hotkey_methods() {
        for message in [
            "Unknown method SetSwitchStyleHotkeyRaw",
            "org.freedesktop.DBus.Error.UnknownMethod",
        ] {
            assert!(tolerate_optional_fcitx_method(Err(BackendError::new(
                BackendErrorCode::Platform,
                message,
            )))
            .is_ok());
        }
    }

    #[test]
    fn optional_hotkey_compatibility_does_not_hide_other_failures() {
        let error = BackendError::new(BackendErrorCode::Platform, "session bus unavailable");
        assert_eq!(
            tolerate_optional_fcitx_method(Err(error.clone())).unwrap_err(),
            error
        );
    }
}
