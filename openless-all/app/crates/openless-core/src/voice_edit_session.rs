//! Core state machine for issue #900's voice edit session.
//!
//! The host owns recording, focus and provider adapters. This module owns the
//! user-visible draft contract: an initial dictated segment is kept separate
//! from the field context, every edit turn is applied deterministically to the
//! preview, and only an explicit commit can produce replacement text.

use serde::{Deserialize, Serialize};

use crate::edit_plan::{apply_edit_plan, EditApplyError, EditPlan};
use crate::types::SessionId;

const MAX_TURNS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextSelection {
    /// UTF-8 character offsets, rather than byte offsets.
    pub start: u32,
    pub end: u32,
}

impl TextSelection {
    pub fn normalized(self) -> (usize, usize) {
        (
            self.start.min(self.end) as usize,
            self.start.max(self.end) as usize,
        )
    }

    pub fn is_non_empty(self) -> bool {
        self.start != self.end
    }

    fn validate(self, field_text: &str) -> Result<(), VoiceEditError> {
        let (start, end) = self.normalized();
        if start == end || end > field_text.chars().count() {
            return Err(VoiceEditError::InvalidSelection);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceEditPhase {
    Dictating,
    DraftReady,
    Editing,
    Applying,
    Preview,
    Committing,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VoiceEditTarget {
    /// Replace only the original selected range.
    Selection { start: u32, end: u32 },
    /// Replace the complete field when the session began at a caret.
    FullField,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceEditContext {
    pub field_text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<TextSelection>,
    pub dictated_segment: String,
    pub draft: String,
    pub preview: String,
    pub turns: Vec<VoiceEditTurn>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceEditTurn {
    pub instruction_raw: String,
    pub instruction_polished: String,
    pub edit_plan: EditPlan,
    pub preview_after: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceEditSnapshot {
    pub session_id: SessionId,
    pub phase: VoiceEditPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<VoiceEditContext>,
    pub target: VoiceEditTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceEditCommit {
    pub session_id: SessionId,
    pub text: String,
    pub target: VoiceEditTarget,
    pub turns: Vec<VoiceEditTurn>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceEditError {
    EmptyDraft,
    EmptyInstruction,
    InvalidSelection,
    InvalidPhase { phase: VoiceEditPhase },
    TooManyTurns,
    EditPlan(EditApplyError),
}

impl std::fmt::Display for VoiceEditError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyDraft => write!(formatter, "voice edit draft is empty"),
            Self::EmptyInstruction => write!(formatter, "voice edit instruction is empty"),
            Self::InvalidSelection => write!(formatter, "voice edit selection is invalid"),
            Self::InvalidPhase { phase } => write!(formatter, "voice edit is in phase {phase:?}"),
            Self::TooManyTurns => write!(formatter, "voice edit has too many turns"),
            Self::EditPlan(error) => write!(formatter, "voice edit plan failed: {error}"),
        }
    }
}

impl std::error::Error for VoiceEditError {}

impl From<EditApplyError> for VoiceEditError {
    fn from(error: EditApplyError) -> Self {
        Self::EditPlan(error)
    }
}

pub struct VoiceEditSession {
    session_id: SessionId,
    phase: VoiceEditPhase,
    context: Option<VoiceEditContext>,
    target: VoiceEditTarget,
}

impl VoiceEditSession {
    /// Start context capture before the initial recording. No draft is usable
    /// and no insertion can happen until [`Self::finish_dictation`] succeeds.
    pub fn start(
        field_text: impl Into<String>,
        selection: Option<TextSelection>,
    ) -> Result<Self, VoiceEditError> {
        let field_text = field_text.into();
        if let Some(selection) = selection {
            selection.validate(&field_text)?;
        }
        let target = selection
            .filter(|selection| selection.is_non_empty())
            .map(|selection| VoiceEditTarget::Selection {
                start: selection.start,
                end: selection.end,
            })
            .unwrap_or(VoiceEditTarget::FullField);
        Ok(Self {
            session_id: SessionId::new(),
            phase: VoiceEditPhase::Dictating,
            context: Some(VoiceEditContext {
                field_text,
                selection,
                dictated_segment: String::new(),
                draft: String::new(),
                preview: String::new(),
                turns: Vec::new(),
            }),
            target,
        })
    }

    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub fn snapshot(&self) -> VoiceEditSnapshot {
        VoiceEditSnapshot {
            session_id: self.session_id,
            phase: self.phase,
            context: self.context.clone(),
            target: self.target,
        }
    }

    /// Finish the no-insert initial recording and construct the first preview.
    pub fn finish_dictation(
        &mut self,
        dictated_segment: impl Into<String>,
    ) -> Result<(), VoiceEditError> {
        self.require_phase(VoiceEditPhase::Dictating)?;
        let dictated_segment = dictated_segment.into();
        let context = self
            .context
            .as_mut()
            .ok_or(VoiceEditError::InvalidPhase { phase: self.phase })?;
        context.dictated_segment = dictated_segment;
        context.draft = merge_draft(
            &context.field_text,
            context.selection,
            &context.dictated_segment,
        )?;
        context.preview = context.draft.clone();
        self.phase = VoiceEditPhase::DraftReady;
        Ok(())
    }

    pub fn enter_editing(&mut self) -> Result<(), VoiceEditError> {
        if !matches!(
            self.phase,
            VoiceEditPhase::DraftReady | VoiceEditPhase::Preview
        ) {
            return Err(VoiceEditError::InvalidPhase { phase: self.phase });
        }
        self.phase = VoiceEditPhase::Editing;
        Ok(())
    }

    /// Mark the model/application part of an instruction turn as in flight.
    /// The host calls this after the no-insert instruction recording has ended
    /// and before it awaits the provider, so the UI can distinguish recording
    /// from an edit plan being applied.
    pub fn begin_applying(&mut self) -> Result<(), VoiceEditError> {
        if self.phase != VoiceEditPhase::Editing {
            return Err(VoiceEditError::InvalidPhase { phase: self.phase });
        }
        self.phase = VoiceEditPhase::Applying;
        Ok(())
    }

    /// Return an interrupted recording or provider turn to the ready draft.
    /// This keeps a transient instruction failure retryable without discarding
    /// the session or inserting anything into the target field.
    pub fn recover_instruction(&mut self) -> Result<(), VoiceEditError> {
        if !matches!(
            self.phase,
            VoiceEditPhase::Editing | VoiceEditPhase::Applying
        ) {
            return Err(VoiceEditError::InvalidPhase { phase: self.phase });
        }
        self.restore_ready_phase()
    }

    /// Apply one already-polished instruction and its validated EditPlan.
    /// Keeping the model call outside this type makes the transition testable
    /// without mocking an internal provider.
    pub fn apply_instruction(
        &mut self,
        instruction_raw: impl Into<String>,
        instruction_polished: impl Into<String>,
        edit_plan: EditPlan,
    ) -> Result<(), VoiceEditError> {
        if !matches!(
            self.phase,
            VoiceEditPhase::DraftReady
                | VoiceEditPhase::Editing
                | VoiceEditPhase::Applying
                | VoiceEditPhase::Preview
        ) {
            return Err(VoiceEditError::InvalidPhase { phase: self.phase });
        }
        let raw = instruction_raw.into();
        let polished = instruction_polished.into();
        if raw.trim().is_empty() || polished.trim().is_empty() {
            return Err(VoiceEditError::EmptyInstruction);
        }
        let preview_before = self
            .context
            .as_ref()
            .ok_or(VoiceEditError::InvalidPhase { phase: self.phase })?
            .preview
            .clone();
        if preview_before.trim().is_empty() {
            return Err(VoiceEditError::EmptyDraft);
        }
        let preview_after = apply_edit_plan(&preview_before, &edit_plan)?;
        let context = self
            .context
            .as_mut()
            .ok_or(VoiceEditError::InvalidPhase { phase: self.phase })?;
        if context.turns.len() >= MAX_TURNS {
            return Err(VoiceEditError::TooManyTurns);
        }
        context.preview = preview_after.clone();
        context.turns.push(VoiceEditTurn {
            instruction_raw: raw,
            instruction_polished: polished,
            edit_plan,
            preview_after,
        });
        self.phase = VoiceEditPhase::Preview;
        Ok(())
    }

    /// Reserve the commit before the host writes the returned text externally.
    pub fn begin_commit(&mut self) -> Result<VoiceEditCommit, VoiceEditError> {
        if !matches!(
            self.phase,
            VoiceEditPhase::DraftReady | VoiceEditPhase::Preview
        ) {
            return Err(VoiceEditError::InvalidPhase { phase: self.phase });
        }
        let context = self
            .context
            .as_ref()
            .ok_or(VoiceEditError::InvalidPhase { phase: self.phase })?;
        if context.preview.trim().is_empty() {
            return Err(VoiceEditError::EmptyDraft);
        }
        self.phase = VoiceEditPhase::Committing;
        Ok(VoiceEditCommit {
            session_id: self.session_id,
            text: context.preview.clone(),
            target: self.target,
            turns: context.turns.clone(),
        })
    }

    /// Finish only after the host has successfully written to the target field.
    pub fn complete_commit(&mut self) -> Result<(), VoiceEditError> {
        self.require_phase(VoiceEditPhase::Committing)?;
        self.phase = VoiceEditPhase::Completed;
        Ok(())
    }

    /// Keep the preview retryable when the host could not write to the target.
    pub fn recover_commit(&mut self) -> Result<(), VoiceEditError> {
        self.require_phase(VoiceEditPhase::Committing)?;
        self.restore_ready_phase()
    }

    pub fn cancel(&mut self) -> Result<(), VoiceEditError> {
        if matches!(
            self.phase,
            VoiceEditPhase::Committing | VoiceEditPhase::Completed | VoiceEditPhase::Cancelled
        ) {
            return Err(VoiceEditError::InvalidPhase { phase: self.phase });
        }
        self.context = None;
        self.phase = VoiceEditPhase::Cancelled;
        Ok(())
    }

    fn restore_ready_phase(&mut self) -> Result<(), VoiceEditError> {
        let context = self
            .context
            .as_ref()
            .ok_or(VoiceEditError::InvalidPhase { phase: self.phase })?;
        self.phase = if context.turns.is_empty() {
            VoiceEditPhase::DraftReady
        } else {
            VoiceEditPhase::Preview
        };
        Ok(())
    }

    fn require_phase(&self, expected: VoiceEditPhase) -> Result<(), VoiceEditError> {
        if self.phase == expected {
            Ok(())
        } else {
            Err(VoiceEditError::InvalidPhase { phase: self.phase })
        }
    }
}

fn merge_draft(
    field_text: &str,
    selection: Option<TextSelection>,
    dictated_segment: &str,
) -> Result<String, VoiceEditError> {
    if let Some(selection) = selection.filter(|selection| selection.is_non_empty()) {
        selection.validate(field_text)?;
        let (start, end) = selection.normalized();
        return slice_chars(field_text, start, end).ok_or(VoiceEditError::InvalidSelection);
    }
    if !field_text.is_empty() {
        return Ok(format!("{field_text}{dictated_segment}"));
    }
    if dictated_segment.trim().is_empty() {
        return Err(VoiceEditError::EmptyDraft);
    }
    Ok(dictated_segment.to_string())
}

fn slice_chars(text: &str, start: usize, end: usize) -> Option<String> {
    let start_byte = if start == 0 {
        0
    } else {
        text.char_indices().nth(start)?.0
    };
    let end_byte = if end == text.chars().count() {
        text.len()
    } else {
        text.char_indices().nth(end)?.0
    };
    Some(text[start_byte..end_byte].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_draft_keeps_caret_text_and_voice_text_without_inventing_spacing() {
        assert_eq!(merge_draft("a", None, "b").unwrap(), "ab");
        assert_eq!(merge_draft("", None, "b").unwrap(), "b");
    }

    #[test]
    fn unicode_selection_uses_character_offsets() {
        assert_eq!(
            merge_draft("甲乙丙", Some(TextSelection { start: 1, end: 3 }), "").unwrap(),
            "乙丙"
        );
    }
}
