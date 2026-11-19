mod image;
pub mod pdf;
mod registry;

pub use image::{decode_image, ImageMime, VerifiedImage};
pub use pdf::{PdfPage, PdfText};
pub use registry::{
    AttachmentLease, AttachmentReader, AttachmentRegistry, AttachmentScope, LeasedAttachment,
    PendingAttachment,
};

use serde::{Deserialize, Serialize};

pub const MAX_ATTACHMENTS_PER_TURN: usize = 4;
pub const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_PDF_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentKind {
    Selection,
    Image,
    Pdf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Png,
    Jpeg,
    Pdf,
}

impl FileKind {
    pub fn max_bytes(self) -> usize {
        match self {
            Self::Png | Self::Jpeg => MAX_IMAGE_BYTES,
            Self::Pdf => MAX_PDF_BYTES,
        }
    }
}

impl From<FileKind> for AttachmentKind {
    fn from(kind: FileKind) -> Self {
        match kind {
            FileKind::Png | FileKind::Jpeg => Self::Image,
            FileKind::Pdf => Self::Pdf,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentState {
    Processing,
    Ready,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentError {
    #[error("The Ask conversation or attachment is no longer available.")]
    NotFound,
    #[error("The attachment is still processing.")]
    NotReady,
    #[error("The attachment list contains a repeated ID.")]
    DuplicateAttachment,
    #[error("At most four attachments can be sent in one turn.")]
    TooManyAttachments,
    #[error("There are too many Ask attachments. Remove an attachment or end a conversation.")]
    CapacityExceeded,
    #[error("The Ask file picker is already open.")]
    PickerBusy,
    #[error("The Ask file picker is unavailable.")]
    PickerUnavailable,
    #[error("Choose a PNG, JPEG, or PDF file.")]
    UnsupportedFile,
    #[error("The selected file could not be read.")]
    FileUnreadable,
    #[error("No text was selected.")]
    EmptySelection,
    #[error("Selected text must fit within 4,000 characters and 16 KiB.")]
    SelectionTooLarge,
    #[error("Images must fit within 8 MiB and 12 megapixels.")]
    ImageTooLarge,
    #[error("The selected image could not be decoded as the chosen PNG or JPEG format.")]
    ImageInvalid,
    #[error("This PDF is encrypted. Choose an unencrypted PDF.")]
    PdfEncrypted,
    #[error("This PDF has no readable text layer. Scans are not converted to text.")]
    PdfNoText,
    #[error("This PDF is damaged or uses unsupported PDF content.")]
    PdfInvalid,
    #[error("This PDF exceeds the allowed size, page, text, or parsing limit.")]
    PdfTooLarge,
    #[error("PDF parsing exceeded the time limit.")]
    PdfTimedOut,
    #[error("The local PDF parser is unavailable.")]
    PdfUnavailable,
    #[error("The attachment was removed or its conversation ended.")]
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentErrorInfo {
    pub code: AttachmentError,
    pub message: String,
}

impl From<AttachmentError> for AttachmentErrorInfo {
    fn from(code: AttachmentError) -> Self {
        Self {
            code,
            message: code.to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AttachmentPreviewSummary {
    Image {
        width: u32,
        height: u32,
        thumbnail_available: bool,
    },
    Selection {
        text: String,
        has_more: bool,
    },
}

#[derive(Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AttachmentPreview {
    Image {
        mime_type: &'static str,
        data_base64: String,
    },
    Selection {
        text: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentMetadata {
    pub attachment_id: String,
    pub kind: AttachmentKind,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<AttachmentPreviewSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_app: Option<String>,
    pub state: AttachmentState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<AttachmentErrorInfo>,
}

pub struct SelectedText {
    text: String,
    source_app: Option<String>,
}

impl SelectedText {
    pub fn new(text: String, source_app: Option<String>) -> Result<Self, AttachmentError> {
        if text.is_empty() {
            return Err(AttachmentError::EmptySelection);
        }
        if text.len() > 16 * 1024 || text.chars().nth(4_000).is_some() {
            return Err(AttachmentError::SelectionTooLarge);
        }
        Ok(Self { text, source_app })
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn source_app(&self) -> Option<&str> {
        self.source_app.as_deref()
    }

    fn summary(&self) -> AttachmentPreviewSummary {
        let end = self
            .text
            .char_indices()
            .nth(280)
            .map_or(self.text.len(), |(index, _)| index);
        AttachmentPreviewSummary::Selection {
            text: self.text[..end].to_owned(),
            has_more: end < self.text.len(),
        }
    }
}

/// Local model material. Deliberately not a public IPC or history DTO.
pub enum AttachmentContent {
    Selection(SelectedText),
    Image(VerifiedImage),
    Pdf(PdfText),
}
