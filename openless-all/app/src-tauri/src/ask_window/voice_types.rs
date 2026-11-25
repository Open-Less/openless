use openless_core::SessionId;
use serde::Deserialize;

#[derive(Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Editor {
    pub(crate) conversation_id: String,
    pub(crate) mount_id: String,
    pub(crate) editor_id: String,
    pub(crate) revision: u64,
    pub(crate) editable: bool,
    pub(crate) selection_start: u32,
    pub(crate) selection_end: u32,
}

#[derive(Clone)]
pub(crate) struct Target {
    pub(crate) focus_epoch: u64,
    pub(crate) editor: Editor,
}

impl Target {
    pub(super) fn matches(&self, current: &Self) -> bool {
        self.focus_epoch == current.focus_epoch && self.editor.editable && self.editor == current.editor
    }
}

#[derive(Clone)]
pub(crate) struct Delivery {
    pub(crate) recording_id: SessionId,
    pub(crate) target: Target,
    pub(crate) text: String,
}
