use std::{collections::HashMap, sync::Arc};

use parking_lot::Mutex;
use tokio::sync::{watch, OwnedSemaphorePermit, Semaphore};
use uuid::Uuid;

use super::{
    AttachmentContent, AttachmentError, AttachmentKind, AttachmentMetadata, AttachmentPreview,
    AttachmentState, FileKind, SelectedText, MAX_ATTACHMENTS_PER_TURN,
};

const MAX_ENTRIES_PER_CONVERSATION: usize = 12;
const MAX_RETAINED_ATTACHMENTS: usize = 24;
const MAX_PROCESSING_JOBS: usize = 2;

pub trait AttachmentReader: Send + Sync {
    fn lease(
        &self,
        conversation_id: &str,
        turn_id: &str,
        attachment_ids: &[String],
    ) -> Result<AttachmentLease, AttachmentError>;
}

pub struct AttachmentLease {
    pub conversation_id: String,
    pub turn_id: String,
    attachments: Vec<LeasedAttachment>,
}

impl AttachmentLease {
    pub fn attachments(&self) -> &[LeasedAttachment] {
        &self.attachments
    }
}

pub struct LeasedAttachment {
    pub attachment_id: String,
    pub display_name: String,
    content: Arc<AttachmentContent>,
    _capacity: Arc<OwnedSemaphorePermit>,
}

impl LeasedAttachment {
    pub fn content(&self) -> &AttachmentContent {
        &self.content
    }
}

pub struct AttachmentRegistry {
    conversations: Mutex<HashMap<String, AttachmentScope>>,
    capacity: Arc<Semaphore>,
    processing: Arc<Semaphore>,
}

impl Default for AttachmentRegistry {
    fn default() -> Self {
        Self {
            conversations: Mutex::new(HashMap::new()),
            capacity: Arc::new(Semaphore::new(MAX_RETAINED_ATTACHMENTS)),
            processing: Arc::new(Semaphore::new(MAX_PROCESSING_JOBS)),
        }
    }
}

impl AttachmentRegistry {
    /// Only the conversation owner calls this, with a newly created conversation ID.
    pub fn open_conversation(&self, conversation_id: &str) -> AttachmentScope {
        self.conversations
            .lock()
            .entry(conversation_id.to_owned())
            .or_insert_with(|| AttachmentScope {
                state: Arc::new(Mutex::new(Some(Vec::new()))),
                capacity: self.capacity.clone(),
                processing: self.processing.clone(),
            })
            .clone()
    }

    pub fn scope(&self, conversation_id: &str) -> Result<AttachmentScope, AttachmentError> {
        self.conversations
            .lock()
            .get(conversation_id)
            .cloned()
            .ok_or(AttachmentError::NotFound)
    }

    pub fn end_conversation(&self, conversation_id: &str) {
        let mut conversations = self.conversations.lock();
        if let Some(scope) = conversations.remove(conversation_id) {
            scope.close();
        }
    }

    pub fn clear(&self) {
        for (_, scope) in self.conversations.lock().drain() {
            scope.close();
        }
    }
}

impl Drop for AttachmentRegistry {
    fn drop(&mut self) {
        self.clear();
    }
}

impl AttachmentReader for AttachmentRegistry {
    fn lease(
        &self,
        conversation_id: &str,
        turn_id: &str,
        attachment_ids: &[String],
    ) -> Result<AttachmentLease, AttachmentError> {
        let scope = self.scope(conversation_id)?;
        if attachment_ids.len() > MAX_ATTACHMENTS_PER_TURN {
            return Err(AttachmentError::TooManyAttachments);
        }
        let state = scope.state.lock();
        let entries = state.as_ref().ok_or(AttachmentError::NotFound)?;
        let mut attachments = Vec::with_capacity(attachment_ids.len());
        for (index, id) in attachment_ids.iter().enumerate() {
            if attachment_ids[..index].contains(id) {
                return Err(AttachmentError::DuplicateAttachment);
            }
            let entry = entries
                .iter()
                .find(|entry| entry.metadata.attachment_id == *id)
                .ok_or(AttachmentError::NotFound)?;
            attachments.push(LeasedAttachment {
                attachment_id: id.clone(),
                display_name: entry.metadata.display_name.clone(),
                content: entry.ready_content()?.clone(),
                _capacity: entry.capacity.clone(),
            });
        }
        Ok(AttachmentLease {
            conversation_id: conversation_id.to_owned(),
            turn_id: turn_id.to_owned(),
            attachments,
        })
    }
}

#[derive(Clone)]
pub struct AttachmentScope {
    state: Arc<Mutex<Option<Vec<Entry>>>>,
    capacity: Arc<Semaphore>,
    processing: Arc<Semaphore>,
}

struct Entry {
    metadata: AttachmentMetadata,
    state: EntryState,
    capacity: Arc<OwnedSemaphorePermit>,
}

enum EntryState {
    Processing { _cancel_on_drop: watch::Sender<()> },
    Ready(Arc<AttachmentContent>),
    Error(AttachmentError),
}

impl Entry {
    fn ready_content(&self) -> Result<&Arc<AttachmentContent>, AttachmentError> {
        match &self.state {
            EntryState::Processing { .. } => Err(AttachmentError::NotReady),
            EntryState::Ready(content) => Ok(content),
            EntryState::Error(error) => Err(*error),
        }
    }

    fn finish(&mut self, result: Result<AttachmentContent, AttachmentError>) {
        match result {
            Ok(content) => {
                self.metadata.state = AttachmentState::Ready;
                match &content {
                    AttachmentContent::Image(image) => {
                        self.metadata.preview = Some(image.summary());
                    }
                    AttachmentContent::Pdf(pdf) => {
                        self.metadata.page_count = Some(pdf.page_count())
                    }
                    AttachmentContent::Selection(selection) => {
                        self.metadata.preview = Some(selection.summary());
                        self.metadata.source_app = selection.source_app.clone();
                    }
                }
                self.state = EntryState::Ready(Arc::new(content));
            }
            Err(error) => {
                self.metadata.state = AttachmentState::Error;
                self.metadata.error = Some(error.into());
                self.state = EntryState::Error(error);
            }
        }
    }
}

impl AttachmentScope {
    pub fn register_selection(
        &self,
        selection: SelectedText,
    ) -> Result<AttachmentMetadata, AttachmentError> {
        let (sender, _) = watch::channel(());
        let mut entry = self.new_entry(
            AttachmentKind::Selection,
            "Selected text".to_owned(),
            sender,
        )?;
        entry.finish(Ok(AttachmentContent::Selection(selection)));
        self.insert(entry)
    }

    pub fn begin_file(
        &self,
        display_name: String,
        kind: FileKind,
    ) -> Result<PendingAttachment, AttachmentError> {
        let (sender, cancellation) = watch::channel(());
        let entry = self.new_entry(kind.into(), display_name, sender)?;
        let metadata = self.insert(entry)?;
        Ok(PendingAttachment {
            scope: self.clone(),
            metadata,
            cancellation,
            finished: false,
        })
    }

    pub fn list(&self) -> Result<Vec<AttachmentMetadata>, AttachmentError> {
        Ok(self
            .state
            .lock()
            .as_ref()
            .ok_or(AttachmentError::NotFound)?
            .iter()
            .map(|entry| entry.metadata.clone())
            .collect())
    }

    pub fn preview(
        &self,
        attachment_id: &str,
    ) -> Result<Option<AttachmentPreview>, AttachmentError> {
        let state = self.state.lock();
        let entry = state
            .as_ref()
            .and_then(|entries| {
                entries
                    .iter()
                    .find(|entry| entry.metadata.attachment_id == attachment_id)
            })
            .ok_or(AttachmentError::NotFound)?;
        Ok(match entry.ready_content()?.as_ref() {
            AttachmentContent::Image(image) => Some(image.preview()),
            AttachmentContent::Selection(selection) => Some(AttachmentPreview::Selection {
                text: selection.text.clone(),
            }),
            AttachmentContent::Pdf(_) => None,
        })
    }

    pub fn remove(&self, attachment_id: &str) -> bool {
        let mut state = self.state.lock();
        if let Some(entries) = state.as_mut() {
            if let Some(index) = entries
                .iter()
                .position(|entry| entry.metadata.attachment_id == attachment_id)
            {
                entries.remove(index);
                return true;
            }
        }
        false
    }

    fn close(&self) {
        self.state.lock().take();
    }

    fn new_entry(
        &self,
        kind: AttachmentKind,
        display_name: String,
        sender: watch::Sender<()>,
    ) -> Result<Entry, AttachmentError> {
        let capacity = self
            .capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| AttachmentError::CapacityExceeded)?;
        Ok(Entry {
            capacity: Arc::new(capacity),
            metadata: AttachmentMetadata {
                attachment_id: Uuid::new_v4().to_string(),
                kind,
                display_name,
                preview: None,
                page_count: None,
                source_app: None,
                state: AttachmentState::Processing,
                error: None,
            },
            state: EntryState::Processing {
                _cancel_on_drop: sender,
            },
        })
    }

    fn insert(&self, entry: Entry) -> Result<AttachmentMetadata, AttachmentError> {
        let mut state = self.state.lock();
        let entries = state.as_mut().ok_or(AttachmentError::NotFound)?;
        if entries.len() >= MAX_ENTRIES_PER_CONVERSATION {
            return Err(AttachmentError::CapacityExceeded);
        }
        let metadata = entry.metadata.clone();
        entries.push(entry);
        Ok(metadata)
    }
}

pub struct PendingAttachment {
    scope: AttachmentScope,
    metadata: AttachmentMetadata,
    cancellation: watch::Receiver<()>,
    finished: bool,
}

impl PendingAttachment {
    pub fn metadata(&self) -> &AttachmentMetadata {
        &self.metadata
    }

    /// Closing the channel cancels processing; no values are sent.
    pub fn cancellation(&self) -> watch::Receiver<()> {
        self.cancellation.clone()
    }

    pub async fn processing_slot(&self) -> Result<OwnedSemaphorePermit, AttachmentError> {
        let mut cancellation = self.cancellation();
        tokio::select! {
            biased;
            _ = cancellation.changed() => Err(AttachmentError::Cancelled),
            slot = self.scope.processing.clone().acquire_owned() => {
                slot.map_err(|_| AttachmentError::Cancelled)
            }
        }
    }

    /// Commit and publish under the host's conversation/event lock. A removed entry cannot reappear.
    pub fn finish(
        mut self,
        result: Result<AttachmentContent, AttachmentError>,
    ) -> Option<AttachmentMetadata> {
        self.finished = true;
        let mut state = self.scope.state.lock();
        let entry = state.as_mut()?.iter_mut().find(|entry| {
            entry.metadata.attachment_id == self.metadata.attachment_id
                && matches!(entry.state, EntryState::Processing { .. })
        })?;
        let result = result.and_then(|content| match (&content, entry.metadata.kind) {
            (AttachmentContent::Image(_), AttachmentKind::Image)
            | (AttachmentContent::Pdf(_), AttachmentKind::Pdf) => Ok(content),
            _ => Err(AttachmentError::UnsupportedFile),
        });
        entry.finish(result);
        Some(entry.metadata.clone())
    }
}

impl Drop for PendingAttachment {
    fn drop(&mut self) {
        if !self.finished {
            self.scope.remove(&self.metadata.attachment_id);
        }
    }
}

#[cfg(test)]
mod tests;
