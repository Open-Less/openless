//! Text-free daily activity aggregates with stable per-device contributions.

use crate::errors::{BackendError, BackendErrorCode};
use crate::persistence::{atomic_write, persistence_error};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const ACTIVITY_RETENTION_DAYS: usize = 731;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DayStats {
    pub count: u32,
    #[serde(default)]
    pub chars: u64,
    #[serde(default)]
    pub duration_ms: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ActivityDay {
    pub date: String,
    pub count: u32,
    pub chars: u64,
    pub duration_ms: u64,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DayBucket {
    #[serde(flatten)]
    stats: DayStats,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    source_contributions: BTreeMap<String, DayStats>,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum StoredDay {
    CountOnly(u32),
    Full(DayBucket),
}

fn totals(sources: &BTreeMap<String, DayStats>) -> DayStats {
    sources
        .values()
        .fold(DayStats::default(), |mut total, source| {
            total.count = total.count.saturating_add(source.count);
            total.chars = total.chars.saturating_add(source.chars);
            total.duration_ms = total.duration_ms.saturating_add(source.duration_ms);
            total
        })
}
fn read_days(path: &Path) -> Result<BTreeMap<String, DayBucket>, BackendError> {
    let raw: BTreeMap<String, serde_json::Value> = match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|_| persistence_error("decode activity aggregates"))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
        Err(_) => return Err(persistence_error("read activity aggregates")),
    };
    let mut stored = BTreeMap::new();
    for (date, value) in raw {
        let day: StoredDay = serde_json::from_value(value.clone())
            .map_err(|_| persistence_error("decode activity day"))?;
        if let StoredDay::Full(bucket) = &day {
            crate::persistence::ensure_lossless_value(
                &value,
                &serde_json::to_value(bucket)
                    .map_err(|_| persistence_error("encode activity day"))?,
                &[],
            )?;
        }
        stored.insert(date, day);
    }
    Ok(stored
        .into_iter()
        .map(|(date, value)| {
            let mut bucket = match value {
                StoredDay::CountOnly(count) => DayBucket {
                    stats: DayStats {
                        count,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                StoredDay::Full(bucket) => bucket,
            };
            if !bucket.source_contributions.is_empty() {
                bucket.stats = totals(&bucket.source_contributions);
            }
            (date, bucket)
        })
        .collect())
}

struct ActivityState {
    days: BTreeMap<String, DayBucket>,
    local_source: Option<String>,
}
pub struct ActivityStore {
    path: Option<PathBuf>,
    cache: Mutex<ActivityState>,
}
impl ActivityStore {
    pub fn at_data_dir(data_dir: impl AsRef<Path>) -> Result<Self, BackendError> {
        Self::at_path(data_dir.as_ref().join("activity.json"))
    }
    pub fn at_path(path: PathBuf) -> Result<Self, BackendError> {
        let days = read_days(&path)?;
        Ok(Self {
            path: Some(path),
            cache: Mutex::new(ActivityState {
                days,
                local_source: None,
            }),
        })
    }
    pub fn in_memory() -> Self {
        Self {
            path: None,
            cache: Mutex::new(ActivityState {
                days: BTreeMap::new(),
                local_source: None,
            }),
        }
    }

    /// Memory-only Host identity attachment; it never attributes imported totals to this device.
    pub(crate) fn bind_sync_device(&self, device: &str) -> Result<(), BackendError> {
        if device.is_empty() {
            return Err(persistence_error("missing activity source device"));
        }
        let mut state = self.lock_cache()?;
        if state
            .local_source
            .as_deref()
            .is_some_and(|old| old != device)
        {
            return Err(persistence_error("activity source identity changed"));
        }
        state.local_source = Some(device.into());
        Ok(())
    }

    pub fn bump(&self, date: &str, chars: u64, duration_ms: u64) -> Result<(), BackendError> {
        crate::cloud_sync_e2ee_store::gate::with_optional_mutation(
            self.path.as_deref(),
            crate::cloud_sync_e2ee_store::gate::ChangeOrigin::User,
            || {
                if !valid_date(date) {
                    return Err(BackendError::new(
                        BackendErrorCode::InvalidArgument,
                        "activity date must use YYYY-MM-DD",
                    ));
                }
                let mut state = self.lock_cache()?;
                let mut days = match &self.path {
                    Some(path) => read_days(path)?,
                    None => state.days.clone(),
                };
                let bucket = days.entry(date.to_string()).or_default();
                if let Some(device) = &state.local_source {
                    if bucket.source_contributions.is_empty() {
                        bucket
                            .source_contributions
                            .insert(device.clone(), bucket.stats);
                    }
                    let own = bucket
                        .source_contributions
                        .entry(device.clone())
                        .or_default();
                    own.count = own.count.saturating_add(1);
                    own.chars = own.chars.saturating_add(chars);
                    own.duration_ms = own.duration_ms.saturating_add(duration_ms);
                    bucket.stats = totals(&bucket.source_contributions);
                } else {
                    if !bucket.source_contributions.is_empty() {
                        return Err(persistence_error(
                            "activity source must be bound before mutation",
                        ));
                    }
                    bucket.stats.count = bucket.stats.count.saturating_add(1);
                    bucket.stats.chars = bucket.stats.chars.saturating_add(chars);
                    bucket.stats.duration_ms = bucket.stats.duration_ms.saturating_add(duration_ms);
                }
                while days.len() > ACTIVITY_RETENTION_DAYS {
                    let Some(oldest) = days.keys().next().cloned() else {
                        break;
                    };
                    days.remove(&oldest);
                }
                if let Some(path) = &self.path {
                    let bytes = serde_json::to_vec_pretty(&days)
                        .map_err(|_| persistence_error("encode activity aggregates"))?;
                    atomic_write(path, &bytes)?;
                }
                state.days = days;
                Ok(())
            },
        )
    }

    pub fn snapshot(&self) -> Result<Vec<ActivityDay>, BackendError> {
        Ok(self
            .lock_cache()?
            .days
            .iter()
            .map(|(date, bucket)| ActivityDay {
                date: date.clone(),
                count: bucket.stats.count,
                chars: bucket.stats.chars,
                duration_ms: bucket.stats.duration_ms,
            })
            .collect())
    }

    pub(crate) fn sync_records(
        &self,
        permit: &crate::cloud_sync_e2ee_store::gate::ExclusivePermit,
    ) -> Result<Vec<crate::cloud_sync_e2ee_documents::ActivityRecord>, BackendError> {
        let path = self
            .path
            .as_deref()
            .ok_or_else(|| persistence_error("activity persistence unavailable"))?;
        crate::cloud_sync_e2ee_store::gate::require_exclusive(path, permit)?;
        self.sync_records_readonly()
    }

    pub(crate) fn sync_records_readonly(
        &self,
    ) -> Result<Vec<crate::cloud_sync_e2ee_documents::ActivityRecord>, BackendError> {
        let path = self
            .path
            .as_deref()
            .ok_or_else(|| persistence_error("activity persistence unavailable"))?;
        let mut state = self.lock_cache()?;
        state.days = read_days(path)?;
        let device = state
            .local_source
            .as_deref()
            .ok_or_else(|| persistence_error("activity source unavailable"))?;
        let mut output = Vec::new();
        for (date, bucket) in &state.days {
            let sources = if bucket.source_contributions.is_empty() {
                BTreeMap::from([(device.to_string(), bucket.stats)])
            } else {
                bucket.source_contributions.clone()
            };
            for (source_device_id, stats) in sources {
                output.push(crate::cloud_sync_e2ee_documents::ActivityRecord {
                    source_device_id,
                    date: date.clone(),
                    count: stats.count.into(),
                    chars: stats.chars,
                    duration_ms: stats.duration_ms,
                });
            }
        }
        Ok(output)
    }

    pub(crate) fn sync_replace_records(
        &self,
        records: &[crate::cloud_sync_e2ee_documents::ActivityRecord],
        permit: &crate::cloud_sync_e2ee_store::gate::ExclusivePermit,
    ) -> Result<(), BackendError> {
        let path = self
            .path
            .as_deref()
            .ok_or_else(|| persistence_error("activity persistence unavailable"))?;
        crate::cloud_sync_e2ee_store::gate::require_exclusive(path, permit)?;
        let mut days: BTreeMap<String, DayBucket> = BTreeMap::new();
        for record in records {
            if !valid_date(&record.date) || record.source_device_id.is_empty() {
                return Err(persistence_error("invalid activity contribution"));
            }
            let stats = DayStats {
                count: u32::try_from(record.count)
                    .map_err(|_| persistence_error("activity count is not representable"))?,
                chars: record.chars,
                duration_ms: record.duration_ms,
            };
            if days
                .entry(record.date.clone())
                .or_default()
                .source_contributions
                .insert(record.source_device_id.clone(), stats)
                .is_some()
            {
                return Err(persistence_error("duplicate activity source"));
            }
        }
        for bucket in days.values_mut() {
            bucket.stats = totals(&bucket.source_contributions);
        }
        // Restoration is complete, independent of the normal rolling retention policy.
        let bytes = serde_json::to_vec_pretty(&days)
            .map_err(|_| persistence_error("encode restored activity"))?;
        let mut state = self.lock_cache()?;
        crate::persistence::atomic_write_for_sync(path, &bytes, permit)?;
        state.days = days;
        Ok(())
    }

    fn lock_cache(&self) -> Result<std::sync::MutexGuard<'_, ActivityState>, BackendError> {
        self.cache.lock().map_err(|_| {
            BackendError::new(BackendErrorCode::Internal, "activity store lock poisoned")
        })
    }
}
fn valid_date(date: &str) -> bool {
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .is_ok_and(|value| value.format("%Y-%m-%d").to_string() == date)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_and_new_entries_round_trip_without_text() {
        let path = std::env::temp_dir().join(format!(
            "openless-core-activity-{}.json",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::write(
            &path,
            br#"{"2026-08-01":5,"2026-08-02":{"count":3,"chars":900,"durationMs":12000}}"#,
        )
        .unwrap();
        let store = ActivityStore::at_path(path.clone()).unwrap();
        assert_eq!(
            store.snapshot().unwrap(),
            vec![
                ActivityDay {
                    date: "2026-08-01".into(),
                    count: 5,
                    chars: 0,
                    duration_ms: 0,
                },
                ActivityDay {
                    date: "2026-08-02".into(),
                    count: 3,
                    chars: 900,
                    duration_ms: 12_000,
                },
            ]
        );
        store.bump("2026-08-02", 100, 500).unwrap();
        assert_eq!(store.snapshot().unwrap()[1].count, 4);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn in_memory_store_validates_dates_and_saturates_totals() {
        let store = ActivityStore::in_memory();
        assert_eq!(
            store.bump("not-a-date", 0, 0).unwrap_err().code,
            BackendErrorCode::InvalidArgument
        );
        store.bump("2026-08-27", u64::MAX, u64::MAX).unwrap();
        store.bump("2026-08-27", 1, 1).unwrap();
        let day = store.snapshot().unwrap().pop().unwrap();
        assert_eq!(day.count, 2);
        assert_eq!(day.chars, u64::MAX);
        assert_eq!(day.duration_ms, u64::MAX);
    }
}
