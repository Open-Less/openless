//! Shared write barrier and durable logical generation. Guards hold no mutex across await.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};

use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::cloud_sync_e2ee_documents::{DocumentError, DocumentResult, SyncScope};
use crate::cloud_sync_e2ee_protocol::types::Revision;

static GATES: OnceLock<Mutex<BTreeMap<PathBuf, Weak<SyncWriteGate>>>> = OnceLock::new();

/// Host entry point before constructing any repository. Reuses the registered Arc exactly.
pub fn open_for_data_dir(data_dir: &Path) -> DocumentResult<Arc<SyncWriteGate>> {
    std::fs::create_dir_all(data_dir.join("encrypted-sync"))
        .map_err(|_| DocumentError::JournalUnavailable)?;
    let root = data_dir
        .canonicalize()
        .map_err(|_| DocumentError::JournalUnavailable)?;
    let mut registry = GATES
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .map_err(|_| DocumentError::RecoveryRequired)?;
    if let Some(gate) = registry.get(&root).and_then(Weak::upgrade) {
        return Ok(gate);
    }
    let gate = SyncWriteGate::open(root.join("encrypted-sync/generation.json"))?;
    registry.insert(root, Arc::downgrade(&gate));
    Ok(gate)
}

fn absolute_normalized(path: &Path) -> DocumentResult<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|_| DocumentError::JournalUnavailable)?
            .join(path)
    };
    let mut output = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !output.pop() {
                    return Err(DocumentError::InvalidDocument);
                }
            }
            value => output.push(value.as_os_str()),
        }
    }
    Ok(output)
}

/// Resolve the nearest existing ancestor, retaining every not-yet-created descendant.
fn resolved_path(path: &Path) -> DocumentResult<PathBuf> {
    let mut ancestor = path.to_path_buf();
    let mut suffix = Vec::new();
    loop {
        match ancestor.canonicalize() {
            Ok(mut resolved) => {
                for part in suffix.iter().rev() {
                    resolved.push(part);
                }
                return absolute_normalized(&resolved);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // A dangling link must not be reinterpreted as an ordinary future directory.
                if std::fs::symlink_metadata(&ancestor)
                    .is_ok_and(|metadata| metadata.file_type().is_symlink())
                {
                    return Err(DocumentError::InvalidDocument);
                }
                suffix.push(
                    ancestor
                        .file_name()
                        .ok_or(DocumentError::InvalidDocument)?
                        .to_os_string(),
                );
                if !ancestor.pop() {
                    return Err(DocumentError::InvalidDocument);
                }
            }
            Err(_) => return Err(DocumentError::JournalUnavailable),
        }
    }
}

pub(crate) fn gate_for_path(path: &Path) -> DocumentResult<Option<Arc<SyncWriteGate>>> {
    if path.as_os_str().is_empty() {
        return Ok(None);
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|_| DocumentError::JournalUnavailable)?
            .join(path)
    };
    let lexical = absolute_normalized(&absolute)?;
    let parent = absolute.parent().map(resolved_path).transpose()?;
    let final_is_symlink = std::fs::symlink_metadata(&absolute)
        .is_ok_and(|metadata| metadata.file_type().is_symlink());
    let resolved = resolved_path(&absolute)?;
    // Root aliases must retain ownership even when a later symlink escapes that root.
    let ancestors: Vec<_> = absolute
        .ancestors()
        .filter_map(|ancestor| ancestor.canonicalize().ok())
        .collect();
    let registry = GATES
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .map_err(|_| DocumentError::RecoveryRequired)?;
    // rename replaces the directory entry, not its symlink target. A managed
    // final-file link must never borrow a different nested root's permit.
    if final_is_symlink
        && parent
            .as_ref()
            .is_some_and(|parent| registry.keys().any(|root| parent.starts_with(root)))
    {
        return Err(DocumentError::InvalidDocument);
    }
    let lexical_root = registry
        .keys()
        .filter(|root| lexical.starts_with(root))
        .max_by_key(|root| root.components().count());
    if lexical_root.is_some_and(|root| !resolved.starts_with(root)) {
        return Err(DocumentError::InvalidDocument);
    }
    for ancestor in &ancestors {
        if registry
            .keys()
            .any(|root| ancestor.starts_with(root) && !resolved.starts_with(root))
        {
            return Err(DocumentError::InvalidDocument);
        }
    }
    let found = registry
        .iter()
        .filter(|(root, _)| resolved.starts_with(root))
        .max_by_key(|(root, _)| root.components().count());
    match found {
        Some((_, weak)) => weak
            .upgrade()
            .map(Some)
            .ok_or(DocumentError::RecoveryRequired),
        None => Ok(None),
    }
}

/// Constructors may parse in memory but must not migrate, salvage-write, or normalize files.
pub fn recovery_pending_for_path(path: &Path) -> bool {
    match gate_for_path(path) {
        Ok(Some(gate)) => gate.recovery_required().unwrap_or(true),
        Ok(None) => false,
        Err(_) => true,
    }
}

struct Observation {
    gate: Arc<SyncWriteGate>,
    wrote: Arc<AtomicBool>,
    dirty: Arc<AtomicBool>,
}
thread_local! { static OBSERVATIONS: RefCell<Vec<Observation>> = const { RefCell::new(Vec::new()) }; }
struct ObservationGuard;
impl Drop for ObservationGuard {
    fn drop(&mut self) {
        OBSERVATIONS.with(|stack| {
            stack.borrow_mut().pop();
        });
    }
}

fn backend_error(error: DocumentError) -> crate::BackendError {
    crate::BackendError::new(
        if error == DocumentError::SourceChanged {
            crate::BackendErrorCode::Busy
        } else {
            crate::BackendErrorCode::InvalidState
        },
        error.to_string(),
    )
}

pub(crate) fn with_registered_mutation<T>(
    path: &Path,
    origin: ChangeOrigin,
    operation: impl FnOnce() -> Result<T, crate::BackendError>,
) -> Result<T, crate::BackendError> {
    with_registered_mutation_deciding(path, || operation().map(|value| (value, origin)))
}

pub(crate) fn with_registered_mutation_deciding<T>(
    path: &Path,
    operation: impl FnOnce() -> Result<(T, ChangeOrigin), crate::BackendError>,
) -> Result<T, crate::BackendError> {
    let Some(gate) = gate_for_path(path).map_err(backend_error)? else {
        return operation().map(|(value, _)| value);
    };
    let permit = gate.begin_mutation().map_err(backend_error)?;
    let wrote = Arc::new(AtomicBool::new(false));
    let dirty = Arc::new(AtomicBool::new(false));
    OBSERVATIONS.with(|stack| {
        stack.borrow_mut().push(Observation {
            gate,
            wrote: Arc::clone(&wrote),
            dirty: Arc::clone(&dirty),
        })
    });
    let observation = ObservationGuard;
    let result = operation();
    drop(observation);
    match result {
        Ok((value, origin)) => {
            if dirty.load(Ordering::Acquire) {
                permit.commit(origin).map_err(backend_error)?;
            } else {
                permit.abort_unmodified().map_err(backend_error)?;
            }
            Ok(value)
        }
        Err(error) => {
            if !wrote.load(Ordering::Acquire)
                && error.code != crate::BackendErrorCode::OutcomeUnknown
            {
                permit.abort_unmodified().map_err(backend_error)?;
            }
            // Otherwise Drop retains the durable uncertain-write intent.
            Err(error)
        }
    }
}

pub(crate) fn with_optional_mutation<T>(
    path: Option<&Path>,
    origin: ChangeOrigin,
    operation: impl FnOnce() -> Result<T, crate::BackendError>,
) -> Result<T, crate::BackendError> {
    match path {
        Some(path) => with_registered_mutation(path, origin, operation),
        None => operation(),
    }
}

fn tracked_file(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some(
            "preferences.json"
                | "history.json"
                | "activity.json"
                | "dictionary.json"
                | "correction-rules.json"
                | "style-packs.json"
                | "vocab-presets.json"
        )
    )
}

pub(crate) fn begin_unobserved_atomic(
    path: &Path,
) -> Result<Option<MutationPermit>, crate::BackendError> {
    // Internal generation/extension files must bypass this lookup to avoid recursion.
    if !tracked_file(path) {
        return Ok(None);
    }
    let Some(gate) = gate_for_path(path).map_err(backend_error)? else {
        return Ok(None);
    };
    let observed = OBSERVATIONS.with(|stack| {
        stack
            .borrow()
            .iter()
            .any(|entry| Arc::ptr_eq(&entry.gate, &gate))
    });
    if observed {
        return Ok(None);
    }
    gate.begin_mutation().map(Some).map_err(backend_error)
}

pub(crate) fn note_successful_write(path: &Path) {
    if !tracked_file(path) {
        return;
    }
    if let Ok(Some(gate)) = gate_for_path(path) {
        OBSERVATIONS.with(|stack| {
            let stack = stack.borrow();
            let matching: Vec<_> = stack
                .iter()
                .filter(|entry| Arc::ptr_eq(&entry.gate, &gate))
                .collect();
            // All ancestors need a durable recovery intent if a later outer step fails.
            for entry in &matching {
                entry.wrote.store(true, Ordering::Release);
            }
            // Only the innermost owner counts this successful persistence.
            if let Some(owner) = matching.last() {
                owner.dirty.store(true, Ordering::Release);
            }
        });
    }
}

pub(crate) fn require_exclusive(
    path: &Path,
    permit: &ExclusivePermit,
) -> Result<(), crate::BackendError> {
    let gate = gate_for_path(path)
        .map_err(backend_error)?
        .ok_or_else(|| backend_error(DocumentError::Unsupported))?;
    if !permit.belongs_to(&gate) {
        return Err(backend_error(DocumentError::InvalidDocument));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChangeOrigin {
    User,
    Restore,
    Recovery,
    LocalOnly,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SyncChange {
    pub generation: Revision,
    pub origin: ChangeOrigin,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GateRestoreReceipt {
    pub operation_id: String,
    pub scope_id: String,
    pub scope: SyncScope,
    pub generation: Revision,
    pub committed: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PendingRestore {
    operation_id: String,
    scope_id: String,
    scope: SyncScope,
    journal_ready: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PersistedState {
    schema_version: u32,
    generation: Revision,
    incomplete_mutations: BTreeSet<String>,
    pending_restore: Option<PendingRestore>,
    completed_restores: BTreeMap<String, GateRestoreReceipt>,
}

struct State {
    stored: PersistedState,
    active: BTreeSet<String>,
    exclusive: bool,
    exclusive_lease: Weak<ExclusiveLease>,
    io_fault: bool,
    epoch: u64,
    pending_change: Option<ChangeOrigin>,
}

pub struct SyncWriteGate {
    path: PathBuf,
    state: Mutex<State>,
    changes: watch::Sender<SyncChange>,
    runtime_blocked: AtomicBool,
}

impl SyncWriteGate {
    /// An unreadable/corrupt counter is an error, never a reset to zero.
    pub fn open(state_path: PathBuf) -> DocumentResult<Arc<Self>> {
        let initialized_path = state_path.with_extension("initialized");
        let stored = match std::fs::read(&state_path) {
            Ok(bytes) => decode(&bytes)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let directory = state_path
                    .parent()
                    .ok_or(DocumentError::JournalUnavailable)?;
                if initialized_path.exists()
                    || (directory.file_name().and_then(|name| name.to_str())
                        == Some("encrypted-sync")
                        && std::fs::read_dir(directory)
                            .map_err(|_| DocumentError::JournalUnavailable)?
                            .next()
                            .is_some())
                {
                    return Err(DocumentError::RecoveryRequired);
                }
                // Create a durable first-initialization sentinel before the initial counter.
                // A crash in this small window fails closed instead of guessing generation zero.
                crate::persistence::atomic_write(&initialized_path, b"openless-sync-generation-v1")
                    .map_err(|_| DocumentError::JournalUnavailable)?;
                let state = PersistedState {
                    schema_version: 1,
                    generation: Revision::new(0),
                    incomplete_mutations: BTreeSet::new(),
                    pending_restore: None,
                    completed_restores: BTreeMap::new(),
                };
                persist(&state_path, &state)?;
                state
            }
            Err(_) => return Err(DocumentError::JournalUnavailable),
        };
        if !initialized_path.exists() {
            crate::persistence::atomic_write(&initialized_path, b"openless-sync-generation-v1")
                .map_err(|_| DocumentError::JournalUnavailable)?;
        }
        let (changes, _) = watch::channel(SyncChange {
            generation: stored.generation,
            origin: ChangeOrigin::Recovery,
        });
        let runtime_blocked = AtomicBool::new(
            !stored.incomplete_mutations.is_empty() || stored.pending_restore.is_some(),
        );
        Ok(Arc::new(Self {
            runtime_blocked,
            path: state_path,
            state: Mutex::new(State {
                stored,
                active: BTreeSet::new(),
                exclusive: false,
                exclusive_lease: Weak::new(),
                io_fault: false,
                epoch: 0,
                pending_change: None,
            }),
            changes,
        }))
    }

    pub fn generation(&self) -> DocumentResult<Revision> {
        let state = self.lock()?;
        if state.io_fault || orphaned(&state) {
            return Err(DocumentError::RecoveryRequired);
        }
        Ok(state.stored.generation)
    }

    /// Optimistic read token; no writes or restore may be in flight at either observation.
    pub fn coherent_generation(&self) -> DocumentResult<(Revision, u64)> {
        let state = self.lock()?;
        if state.io_fault || orphaned(&state) || state.stored.pending_restore.is_some() {
            return Err(DocumentError::RecoveryRequired);
        }
        if state.exclusive || !state.active.is_empty() {
            return Err(DocumentError::SourceChanged);
        }
        Ok((state.stored.generation, state.epoch))
    }

    pub fn subscribe(&self) -> watch::Receiver<SyncChange> {
        self.changes.subscribe()
    }

    /// Public metadata only; recovery visits these registered scopes, never scans user files.
    pub fn registered_recovery_scopes(&self) -> DocumentResult<Vec<SyncScope>> {
        let state = self.lock()?;
        let mut scopes = Vec::new();
        if let Some(pending) = &state.stored.pending_restore {
            scopes.push(pending.scope.clone());
        }
        for receipt in state.stored.completed_restores.values() {
            if !scopes.contains(&receipt.scope) {
                scopes.push(receipt.scope.clone());
            }
        }
        Ok(scopes)
    }

    /// Runtime hot paths never wait behind journal I/O or the write-state mutex.
    pub fn recovery_required(&self) -> DocumentResult<bool> {
        Ok(self.runtime_blocked.load(Ordering::Acquire))
    }

    fn refresh_runtime_flag(&self, state: &State) {
        self.runtime_blocked.store(
            state.io_fault || orphaned(state) || state.stored.pending_restore.is_some(),
            Ordering::Release,
        );
    }

    /// Acquire before reading data for a read-modify-write. Only commit after its save succeeds.
    pub fn begin_mutation(self: &Arc<Self>) -> DocumentResult<MutationPermit> {
        let mut state = self.lock()?;
        if state.io_fault || orphaned(&state) || state.stored.pending_restore.is_some() {
            return Err(DocumentError::RecoveryRequired);
        }
        if state.exclusive {
            return Err(DocumentError::SourceChanged);
        }
        let id = uuid::Uuid::new_v4().to_string();
        let mut next = state.stored.clone();
        next.incomplete_mutations.insert(id.clone());
        self.save(&mut state, next)?;
        state.epoch = state
            .epoch
            .checked_add(1)
            .ok_or(DocumentError::RecoveryRequired)?;
        state.active.insert(id.clone());
        self.refresh_runtime_flag(&state);
        Ok(MutationPermit {
            gate: Arc::clone(self),
            id,
            finished: false,
        })
    }

    /// Fail fast while any mutation is active; never block a writer's nested operation.
    pub fn try_exclusive(self: &Arc<Self>) -> DocumentResult<ExclusivePermit> {
        let mut state = self.lock()?;
        if state.io_fault || orphaned(&state) || state.stored.pending_restore.is_some() {
            return Err(DocumentError::RecoveryRequired);
        }
        if state.exclusive || !state.active.is_empty() {
            return Err(DocumentError::SourceChanged);
        }
        state.epoch = state
            .epoch
            .checked_add(1)
            .ok_or(DocumentError::RecoveryRequired)?;
        state.exclusive = true;
        let lease = Arc::new(ExclusiveLease {
            gate: Arc::clone(self),
        });
        state.exclusive_lease = Arc::downgrade(&lease);
        Ok(ExclusivePermit { lease })
    }

    /// For startup journal recovery or a strict full-store reconciliation after an uncertain save.
    pub fn try_exclusive_recovery(self: &Arc<Self>) -> DocumentResult<ExclusivePermit> {
        let mut state = self.lock()?;
        if state.exclusive || !state.active.is_empty() {
            return Err(DocumentError::SourceChanged);
        }
        let stored =
            decode(&std::fs::read(&self.path).map_err(|_| DocumentError::JournalUnavailable)?)?;
        state.stored = stored;
        state.io_fault = false;
        self.refresh_runtime_flag(&state);
        state.epoch = state
            .epoch
            .checked_add(1)
            .ok_or(DocumentError::RecoveryRequired)?;
        state.exclusive = true;
        let lease = Arc::new(ExclusiveLease {
            gate: Arc::clone(self),
        });
        state.exclusive_lease = Arc::downgrade(&lease);
        Ok(ExclusivePermit { lease })
    }

    pub(crate) fn clone_exclusive_for_metadata(&self) -> DocumentResult<ExclusivePermit> {
        self.lock()?
            .exclusive_lease
            .upgrade()
            .map(|lease| ExclusivePermit { lease })
            .ok_or(DocumentError::SourceChanged)
    }

    fn lock(&self) -> DocumentResult<MutexGuard<'_, State>> {
        self.state.lock().map_err(|_| {
            self.runtime_blocked.store(true, Ordering::Release);
            DocumentError::RecoveryRequired
        })
    }

    fn save(&self, state: &mut State, next: PersistedState) -> DocumentResult<()> {
        if let Err(error) = persist(&self.path, &next) {
            state.io_fault = true;
            self.runtime_blocked.store(true, Ordering::Release);
            return Err(error);
        }
        state.stored = next;
        Ok(())
    }

    fn publish(&self, generation: Revision, origin: ChangeOrigin) {
        if origin != ChangeOrigin::LocalOnly {
            // Concurrent commits can reach notification delivery out of order.
            self.changes.send_if_modified(|current| {
                if generation < current.generation
                    || *current == (SyncChange { generation, origin })
                {
                    return false;
                }
                *current = SyncChange { generation, origin };
                true
            });
        }
    }
}

fn orphaned(state: &State) -> bool {
    state.stored.incomplete_mutations != state.active
}

fn decode(bytes: &[u8]) -> DocumentResult<PersistedState> {
    if bytes.len() > 1024 * 1024 {
        return Err(DocumentError::JournalUnavailable);
    }
    let state: PersistedState =
        serde_json::from_slice(bytes).map_err(|_| DocumentError::JournalUnavailable)?;
    if state.schema_version != 1 {
        return Err(DocumentError::Unsupported);
    }
    Ok(state)
}

fn persist(path: &std::path::Path, state: &PersistedState) -> DocumentResult<()> {
    let bytes = serde_json::to_vec(state).map_err(|_| DocumentError::JournalUnavailable)?;
    if bytes.len() > 1024 * 1024 {
        return Err(DocumentError::JournalUnavailable);
    }
    crate::persistence::atomic_write(path, &bytes).map_err(|_| DocumentError::JournalUnavailable)
}

/// Owned and Send; keep it inside the actual task, not only its caller's cancellable future.
pub struct MutationPermit {
    gate: Arc<SyncWriteGate>,
    id: String,
    finished: bool,
}

impl MutationPermit {
    pub fn commit(mut self, origin: ChangeOrigin) -> DocumentResult<Revision> {
        let (generation, publish) = {
            let mut state = self.gate.lock()?;
            if !state.active.contains(&self.id) || state.io_fault {
                return Err(DocumentError::RecoveryRequired);
            }
            let mut next = state.stored.clone();
            if origin != ChangeOrigin::LocalOnly {
                next.generation = next
                    .generation
                    .checked_next()
                    .map_err(|_| DocumentError::RecoveryRequired)?;
            }
            next.incomplete_mutations.remove(&self.id);
            self.gate.save(&mut state, next)?;
            state.active.remove(&self.id);
            self.gate.refresh_runtime_flag(&state);
            if origin != ChangeOrigin::LocalOnly {
                state.pending_change = Some(origin);
            }
            let publish = if state.active.is_empty() {
                state.pending_change.take()
            } else {
                None
            };
            self.finished = true;
            (state.stored.generation, publish)
        };
        if let Some(origin) = publish {
            self.gate.publish(generation, origin);
        }
        Ok(generation)
    }

    /// Only for a validation/no-op/error path known not to have persisted anything.
    pub fn abort_unmodified(mut self) -> DocumentResult<()> {
        let (generation, publish) = {
            let mut state = self.gate.lock()?;
            let mut next = state.stored.clone();
            next.incomplete_mutations.remove(&self.id);
            self.gate.save(&mut state, next)?;
            state.active.remove(&self.id);
            self.gate.refresh_runtime_flag(&state);
            let publish = if state.active.is_empty() {
                state.pending_change.take()
            } else {
                None
            };
            self.finished = true;
            (state.stored.generation, publish)
        };
        if let Some(origin) = publish {
            self.gate.publish(generation, origin);
        }
        Ok(())
    }
}

impl Drop for MutationPermit {
    fn drop(&mut self) {
        if !self.finished {
            if let Ok(mut state) = self.gate.state.lock() {
                state.active.remove(&self.id);
                self.gate.refresh_runtime_flag(&state);
                // The durable intent remains until an actual coherent capture reconciles state.
                state.io_fault = true;
                self.gate.runtime_blocked.store(true, Ordering::Release);
            }
        }
    }
}

#[derive(Clone)]
pub struct ExclusivePermit {
    lease: Arc<ExclusiveLease>,
}
struct ExclusiveLease {
    gate: Arc<SyncWriteGate>,
}

impl ExclusivePermit {
    pub fn belongs_to(&self, gate: &Arc<SyncWriteGate>) -> bool {
        Arc::ptr_eq(&self.lease.gate, gate)
    }

    pub fn generation(&self) -> DocumentResult<Revision> {
        Ok(self.lease.gate.lock()?.stored.generation)
    }

    pub fn pending_restore(&self) -> DocumentResult<bool> {
        Ok(self.lease.gate.lock()?.stored.pending_restore.is_some())
    }

    /// Called only after every store and credential source was successfully read/validated.
    /// It records an observed recovery epoch, never declares an interrupted mutation successful.
    pub(crate) fn accept_reconciled_capture(&self) -> DocumentResult<Revision> {
        let generation = {
            let mut state = self.lease.gate.lock()?;
            if state.stored.pending_restore.is_some() {
                return Err(DocumentError::RecoveryRequired);
            }
            if state.stored.incomplete_mutations.is_empty() {
                return Ok(state.stored.generation);
            }
            let mut next = state.stored.clone();
            next.generation = next
                .generation
                .checked_next()
                .map_err(|_| DocumentError::RecoveryRequired)?;
            next.incomplete_mutations.clear();
            self.lease.gate.save(&mut state, next)?;
            self.lease.gate.refresh_runtime_flag(&state);
            state.stored.generation
        };
        self.lease.gate.publish(generation, ChangeOrigin::Recovery);
        Ok(generation)
    }

    pub fn mark_restore_pending(
        &self,
        operation_id: &str,
        scope_id: &str,
        scope: &SyncScope,
    ) -> DocumentResult<()> {
        let mut state = self.lease.gate.lock()?;
        if let Some(pending) = &state.stored.pending_restore {
            if pending.operation_id != operation_id
                || pending.scope_id != scope_id
                || &pending.scope != scope
            {
                return Err(DocumentError::RecoveryRequired);
            }
            return Ok(());
        }
        let mut next = state.stored.clone();
        next.pending_restore = Some(PendingRestore {
            operation_id: operation_id.into(),
            scope_id: scope_id.into(),
            scope: scope.clone(),
            journal_ready: false,
        });
        self.lease.gate.save(&mut state, next)?;
        self.lease.gate.refresh_runtime_flag(&state);
        Ok(())
    }

    pub fn mark_journal_ready(&self, operation_id: &str, scope_id: &str) -> DocumentResult<()> {
        let mut state = self.lease.gate.lock()?;
        let mut next = state.stored.clone();
        let pending = next
            .pending_restore
            .as_mut()
            .ok_or(DocumentError::RecoveryRequired)?;
        if pending.operation_id != operation_id || pending.scope_id != scope_id {
            return Err(DocumentError::RecoveryRequired);
        }
        pending.journal_ready = true;
        self.lease.gate.save(&mut state, next)?;
        self.lease.gate.refresh_runtime_flag(&state);
        Ok(())
    }

    pub fn recover_without_journal(&self, scope_id: &str) -> DocumentResult<()> {
        let operation = {
            let state = self.lease.gate.lock()?;
            let Some(pending) = &state.stored.pending_restore else {
                return Ok(());
            };
            if pending.scope_id != scope_id || pending.journal_ready {
                return Err(DocumentError::RecoveryRequired);
            }
            pending.operation_id.clone()
        };
        self.finish_restore(&operation, scope_id, false).map(|_| ())
    }

    pub fn completed_restore(
        &self,
        operation_id: &str,
        scope_id: &str,
    ) -> DocumentResult<Option<GateRestoreReceipt>> {
        Ok(self
            .lease
            .gate
            .lock()?
            .stored
            .completed_restores
            .get(scope_id)
            .filter(|receipt| receipt.operation_id == operation_id)
            .cloned())
    }

    pub(crate) fn forget_completed_scope_without_journal(
        &self,
        scope_id: &str,
    ) -> DocumentResult<()> {
        let mut state = self.lease.gate.lock()?;
        if state
            .stored
            .pending_restore
            .as_ref()
            .is_some_and(|pending| pending.scope_id == scope_id)
        {
            return Err(DocumentError::RecoveryRequired);
        }
        if state.stored.completed_restores.contains_key(scope_id) {
            let mut next = state.stored.clone();
            next.completed_restores.remove(scope_id);
            self.lease.gate.save(&mut state, next)?;
        }
        Ok(())
    }

    pub(crate) fn forget_completed_restore(
        &self,
        operation_id: &str,
        scope_id: &str,
    ) -> DocumentResult<()> {
        let mut state = self.lease.gate.lock()?;
        if state
            .stored
            .completed_restores
            .get(scope_id)
            .is_some_and(|receipt| receipt.operation_id == operation_id)
        {
            let mut next = state.stored.clone();
            next.completed_restores.remove(scope_id);
            self.lease.gate.save(&mut state, next)?;
        }
        Ok(())
    }

    pub fn finish_restore(
        &self,
        operation_id: &str,
        scope_id: &str,
        committed: bool,
    ) -> DocumentResult<GateRestoreReceipt> {
        if let Some(receipt) = self.completed_restore(operation_id, scope_id)? {
            if receipt.committed != committed {
                return Err(DocumentError::RecoveryRequired);
            }
            return Ok(receipt);
        }
        let receipt = {
            let mut state = self.lease.gate.lock()?;
            let pending = state
                .stored
                .pending_restore
                .as_ref()
                .ok_or(DocumentError::RecoveryRequired)?;
            if pending.operation_id != operation_id || pending.scope_id != scope_id {
                return Err(DocumentError::RecoveryRequired);
            }
            let scope = pending.scope.clone();
            let mut next = state.stored.clone();
            if committed {
                next.generation = next
                    .generation
                    .checked_next()
                    .map_err(|_| DocumentError::RecoveryRequired)?;
            }
            next.pending_restore = None;
            let receipt = GateRestoreReceipt {
                operation_id: operation_id.into(),
                scope_id: scope_id.into(),
                scope,
                generation: next.generation,
                committed,
            };
            next.completed_restores
                .insert(scope_id.to_string(), receipt.clone());
            self.lease.gate.save(&mut state, next)?;
            self.lease.gate.refresh_runtime_flag(&state);
            receipt
        };
        if committed {
            self.lease
                .gate
                .publish(receipt.generation, ChangeOrigin::Restore);
        }
        Ok(receipt)
    }
}

impl Drop for ExclusiveLease {
    fn drop(&mut self) {
        if let Ok(mut state) = self.gate.state.lock() {
            state.exclusive = false;
            match state.epoch.checked_add(1) {
                Some(next) => state.epoch = next,
                None => {
                    state.io_fault = true;
                    self.gate.runtime_blocked.store(true, Ordering::Release);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("sync-gate-fixture-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn nested_completion_flushes_only_after_last_abort_or_local_only_commit() {
        for local_only in [false, true] {
            let temp = Temp::new();
            let gate = open_for_data_dir(&temp.0).unwrap();
            let changes = gate.subscribe();
            let outer = gate.begin_mutation().unwrap();
            let inner = gate.begin_mutation().unwrap();
            inner.commit(ChangeOrigin::User).unwrap();
            assert!(!changes.has_changed().unwrap());
            if local_only {
                outer.commit(ChangeOrigin::LocalOnly).unwrap();
            } else {
                outer.abort_unmodified().unwrap();
            }
            assert!(changes.has_changed().unwrap());
            assert_eq!(changes.borrow().generation, gate.generation().unwrap());
        }
    }
    #[test]
    fn exclusive_clones_hold_barrier_and_rollback_changes_capture_epoch() {
        let temp = Temp::new();
        let gate = open_for_data_dir(&temp.0).unwrap();
        let before = gate.coherent_generation().unwrap();
        let first = gate.try_exclusive().unwrap();
        let last = first.clone();
        drop(first);
        assert!(gate.begin_mutation().is_err());
        drop(last);
        let after = gate.coherent_generation().unwrap();
        assert_eq!(before.0, after.0);
        assert_ne!(before.1, after.1);
    }
    #[test]
    fn delayed_old_notification_cannot_move_watch_backwards() {
        let temp = Temp::new();
        let gate = open_for_data_dir(&temp.0).unwrap();
        gate.publish(Revision::new(2), ChangeOrigin::User);
        gate.publish(Revision::new(1), ChangeOrigin::User);
        assert_eq!(gate.subscribe().borrow().generation, Revision::new(2));
    }
    #[cfg(unix)]
    #[test]
    fn aliases_missing_descendants_and_escaping_symlinks_cannot_bypass_gate() {
        let temp = Temp::new();
        let root = temp.0.join("data");
        std::fs::create_dir(&root).unwrap();
        let gate = open_for_data_dir(&root).unwrap();
        let alias = temp.0.join("alias");
        std::os::unix::fs::symlink(&root, &alias).unwrap();
        let permit = gate.try_exclusive().unwrap();
        for path in [
            alias.join("preferences.json"),
            alias.join("missing/nested/preferences.json"),
        ] {
            assert!(with_registered_mutation(&path, ChangeOrigin::User, || {
                crate::persistence::atomic_write(&path, b"{}")
            })
            .is_err());
            assert!(!path.exists());
        }
        std::os::unix::fs::symlink(temp.0.join("outside"), root.join("escape")).unwrap();
        assert!(gate_for_path(&root.join("escape/preferences.json")).is_err());
        drop(permit);
        assert_eq!(gate.generation().unwrap(), Revision::new(0));
    }
    #[test]
    fn unfinished_mutation_survives_reopen_and_only_reconciliation_clears_it() {
        let temp = Temp::new();
        let path = temp.0.join("state.json");
        let gate = SyncWriteGate::open(path.clone()).unwrap();
        drop(gate.begin_mutation().unwrap());
        drop(gate);
        let gate = SyncWriteGate::open(path).unwrap();
        assert!(gate.recovery_required().unwrap());
        let permit = gate.try_exclusive_recovery().unwrap();
        permit.accept_reconciled_capture().unwrap();
        drop(permit);
        assert_eq!(gate.generation().unwrap(), Revision::new(1));
        assert!(!gate.recovery_required().unwrap());
    }
    #[test]
    fn persist_rejects_unreadable_size_before_replacing_existing_state() {
        let temp = Temp::new();
        let gate = open_for_data_dir(&temp.0).unwrap();
        let path = temp.0.join("encrypted-sync/generation.json");
        let before = std::fs::read(&path).unwrap();
        let mut state = gate.lock().unwrap().stored.clone();
        state.incomplete_mutations.insert("x".repeat(1024 * 1024));
        assert!(persist(&path, &state).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert!(decode(&before).is_ok());
    }
    #[test]
    fn lost_counter_never_resets_an_initialized_gate_or_existing_protected_state() {
        let temp = Temp::new();
        let gate = open_for_data_dir(&temp.0).unwrap();
        let path = temp.0.join("encrypted-sync/generation.json");
        gate.begin_mutation()
            .unwrap()
            .commit(ChangeOrigin::User)
            .unwrap();
        drop(gate);
        std::fs::remove_file(&path).unwrap();
        assert!(matches!(
            SyncWriteGate::open(path),
            Err(DocumentError::RecoveryRequired)
        ));
        let other = Temp::new();
        std::fs::create_dir(other.0.join("encrypted-sync")).unwrap();
        std::fs::write(other.0.join("encrypted-sync/fixture.enc"), b"ciphertext").unwrap();
        assert!(matches!(
            open_for_data_dir(&other.0),
            Err(DocumentError::RecoveryRequired)
        ));
    }

    #[test]
    fn runtime_recovery_flag_does_not_wait_for_state_lock_or_file_io() {
        let temp = Temp::new();
        let gate = open_for_data_dir(&temp.0).unwrap();
        let held = gate.state.lock().unwrap();
        assert!(!gate.recovery_required().unwrap());
        drop(held);
        drop(gate.begin_mutation().unwrap());
        let held = gate.state.lock().unwrap();
        assert!(gate.recovery_required().unwrap());
        drop(held);
    }
    #[test]
    fn inner_success_followed_by_outer_failure_keeps_an_uncertain_outer_intent() {
        let temp = Temp::new();
        let gate = open_for_data_dir(&temp.0).unwrap();
        let path = temp.0.join("preferences.json");
        let result: Result<(), crate::BackendError> =
            with_registered_mutation(&path, ChangeOrigin::User, || {
                with_registered_mutation(&path, ChangeOrigin::User, || {
                    crate::persistence::atomic_write(&path, b"{}")
                })?;
                Err(crate::BackendError::new(
                    crate::BackendErrorCode::Persistence,
                    "later outer step failed",
                ))
            });
        assert!(result.is_err());
        assert!(gate.recovery_required().unwrap());
        assert_eq!(std::fs::read(path).unwrap(), b"{}");
        let permit = gate.try_exclusive_recovery().unwrap();
        assert_eq!(permit.generation().unwrap(), Revision::new(1));
        assert!(!gate.lock().unwrap().stored.incomplete_mutations.is_empty());
    }
    #[cfg(unix)]
    #[test]
    fn aliased_root_with_final_or_intermediate_escape_cannot_bypass_held_exclusive() {
        let temp = Temp::new();
        let root = temp.0.join("data");
        std::fs::create_dir(&root).unwrap();
        let gate = open_for_data_dir(&root).unwrap();
        let alias = temp.0.join("alias");
        std::os::unix::fs::symlink(&root, &alias).unwrap();
        let outside = temp.0.join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("preferences.json"), b"outside").unwrap();
        std::os::unix::fs::symlink(
            outside.join("preferences.json"),
            root.join("preferences.json"),
        )
        .unwrap();
        std::os::unix::fs::symlink(&outside, root.join("escape")).unwrap();
        let _permit = gate.try_exclusive().unwrap();
        for path in [
            alias.join("preferences.json"),
            alias.join("escape/preferences.json"),
        ] {
            assert!(with_registered_mutation(&path, ChangeOrigin::User, || {
                crate::persistence::atomic_write(&path, b"changed")
            })
            .is_err());
        }
        assert_eq!(
            std::fs::read(outside.join("preferences.json")).unwrap(),
            b"outside"
        );
        assert_eq!(gate.generation().unwrap(), Revision::new(0));
    }
    #[cfg(unix)]
    #[test]
    fn managed_final_link_into_a_nested_registered_root_never_borrows_its_permit() {
        let temp = Temp::new();
        let outer = open_for_data_dir(&temp.0).unwrap();
        let nested = temp.0.join("child");
        let inner = open_for_data_dir(&nested).unwrap();
        let target = nested.join("existing.json");
        std::fs::write(&target, b"child").unwrap();
        let link = temp.0.join("preferences.json");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let _permit = outer.try_exclusive().unwrap();
        assert!(with_registered_mutation(&link, ChangeOrigin::User, || {
            crate::persistence::atomic_write(&link, b"outer")
        })
        .is_err());
        assert!(std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(std::fs::read(target).unwrap(), b"child");
        assert_eq!(outer.generation().unwrap(), Revision::new(0));
        assert_eq!(inner.generation().unwrap(), Revision::new(0));
    }
}
