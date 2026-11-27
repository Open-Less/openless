//! Request-only document data. Native identity and offsets never belong here.

use serde::{Deserialize, Serialize};

const WIRE_PREFIX: &str = "OPENLESS_DOCUMENT_CONTEXT_V1\n";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CursorContext {
    pub before: String,
    pub after: String,
    /// The complete original selection, separate from reference text. Empty
    /// means an insertion point. Selection editing still uses its #8 input.
    pub selection: String,
}

impl CursorContext {
    pub fn is_empty(&self) -> bool {
        self.before.is_empty() && self.after.is_empty() && self.selection.is_empty()
    }

    /// Bridge to older provider signatures without collapsing the three
    /// fields or adding native metadata. Struct field order is deterministic.
    pub fn request_input(&self) -> String {
        format!(
            "{WIRE_PREFIX}{}",
            serde_json::to_string(self).expect("document strings serialize")
        )
    }

    pub(crate) fn from_request_input(value: &str) -> Option<Self> {
        serde_json::from_str(value.strip_prefix(WIRE_PREFIX)?).ok()
    }
}
