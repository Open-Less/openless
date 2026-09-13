//! Mobile stub — shortcut binding validation is unavailable on mobile.

use crate::types::ShortcutBinding;

pub use openless_core::{
    binding_requires_side_aware_hook, bindings_overlap, is_side_specific_modifier_tag,
    legacy_modifier_trigger, normalize_side_modifier_tag, reject_side_specific_non_dictation,
    SIDE_SPECIFIC_NON_DICTATION_MSG,
};

#[derive(Debug, thiserror::Error)]
pub enum ShortcutBindingError {
    #[error("快捷键在移动端不可用")]
    Unavailable,
}

pub fn validate_binding(_binding: &ShortcutBinding) -> Result<(), ShortcutBindingError> {
    Err(ShortcutBindingError::Unavailable)
}

pub fn parse_global_hotkey(_binding: &ShortcutBinding) -> Result<(), ShortcutBindingError> {
    Err(ShortcutBindingError::Unavailable)
}
