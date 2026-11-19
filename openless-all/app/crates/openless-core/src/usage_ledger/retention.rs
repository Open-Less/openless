//! Deterministic union and bounded serialization, without ledger I/O or generation changes.

use std::collections::{hash_map::Entry, HashMap};

use super::UsageRecord;
use crate::errors::BackendError;
use crate::persistence::persistence_error;
use uuid::Uuid;

pub(super) const MAX_RECORDS: usize = 10_000;
pub(super) const MAX_BYTES: usize = 8 * 1024 * 1024;
const DOCUMENT_PREFIX: &[u8] = br#"{"version":1,"records":["#;
const DOCUMENT_SUFFIX: &[u8] = b"]}";
pub(super) const EMPTY_DOCUMENT: &[u8] = br#"{"version":1,"records":[]}"#;

pub(super) fn merge_retained_records(
    current: &[UsageRecord],
    records: impl IntoIterator<Item = UsageRecord>,
) -> Result<Option<(Vec<UsageRecord>, Vec<u8>)>, BackendError> {
    let mut joined: HashMap<Uuid, UsageRecord> = current
        .iter()
        .map(|record| (record.id, record.clone()))
        .collect();
    let mut added = false;
    for record in records {
        if !record.valid() {
            return Err(persistence_error("invalid service usage record"));
        }
        match joined.entry(record.id) {
            Entry::Occupied(entry) => {
                if entry.get() != &record {
                    return Err(persistence_error("conflicting service usage record"));
                }
            }
            Entry::Vacant(entry) => {
                entry.insert(record);
                added = true;
            }
        }
    }
    if !added {
        return Ok(None);
    }
    let mut next: Vec<_> = joined.into_values().collect();
    let bytes = encode_retained_records(&mut next)?;
    if next.as_slice() == current {
        return Ok(None);
    }
    Ok(Some((next, bytes)))
}

pub(super) fn retention_key(record: &UsageRecord) -> (i64, Uuid) {
    (record.occurred_at_ms, record.id)
}

fn encode_retained_records(records: &mut Vec<UsageRecord>) -> Result<Vec<u8>, BackendError> {
    // A deterministic newest suffix also makes an evicted replay a no-op while
    // its newer records remain. A time tie cannot depend on insertion order.
    records.sort_unstable_by_key(retention_key);
    records.drain(..records.len().saturating_sub(MAX_RECORDS));
    let mut bytes = DOCUMENT_PREFIX.to_vec();
    let mut starts = Vec::with_capacity(records.len());
    for (index, record) in records.iter().enumerate() {
        if index != 0 {
            bytes.push(b',');
        }
        starts.push(bytes.len());
        serde_json::to_writer(&mut bytes, record)
            .map_err(|_| persistence_error("encode service usage"))?;
    }
    let keep_from = starts.partition_point(|start| {
        DOCUMENT_PREFIX.len() + bytes.len() - start + DOCUMENT_SUFFIX.len() > MAX_BYTES
    });
    if let Some(&start) = starts.get(keep_from) {
        if keep_from > 0 {
            bytes.copy_within(start.., DOCUMENT_PREFIX.len());
            bytes.truncate(DOCUMENT_PREFIX.len() + bytes.len() - start);
        }
    } else {
        bytes.truncate(DOCUMENT_PREFIX.len());
    }
    records.drain(..keep_from);
    bytes.extend_from_slice(DOCUMENT_SUFFIX);
    Ok(bytes)
}
