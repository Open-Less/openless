//! Text-free daily activity aggregates shared by both desktop hosts.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::errors::{BackendError, BackendErrorCode};
use crate::persistence::{atomic_write, persistence_error};

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

#[derive(Deserialize)]
#[serde(untagged)]
enum StoredDay {
    CountOnly(u32),
    Full(DayStats),
}

impl From<StoredDay> for DayStats {
    fn from(stored: StoredDay) -> Self {
        match stored {
            StoredDay::CountOnly(count) => Self {
                count,
                ..Self::default()
            },
            StoredDay::Full(stats) => stats,
        }
    }
}

pub struct ActivityStore {
    path: Option<PathBuf>,
    cache: Mutex<BTreeMap<String, DayStats>>,
}

impl ActivityStore {
    pub fn at_data_dir(data_dir: impl AsRef<Path>) -> Result<Self, BackendError> {
        Self::at_path(data_dir.as_ref().join("activity.json"))
    }

    pub fn at_path(path: PathBuf) -> Result<Self, BackendError> {
        let stored = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<BTreeMap<String, StoredDay>>(&bytes)
                .map_err(|_| persistence_error("decode activity aggregates"))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(_) => return Err(persistence_error("read activity aggregates")),
        };
        Ok(Self {
            path: Some(path),
            cache: Mutex::new(
                stored
                    .into_iter()
                    .map(|(date, day)| (date, day.into()))
                    .collect(),
            ),
        })
    }

    /// In-memory degradation for a non-critical aggregate store.
    pub fn in_memory() -> Self {
        Self {
            path: None,
            cache: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn bump(&self, date: &str, chars: u64, duration_ms: u64) -> Result<(), BackendError> {
        if !valid_date(date) {
            return Err(BackendError::new(
                BackendErrorCode::InvalidArgument,
                "activity date must use YYYY-MM-DD",
            ));
        }
        let mut cache = self.lock_cache()?;
        let entry = cache.entry(date.to_string()).or_default();
        entry.count = entry.count.saturating_add(1);
        entry.chars = entry.chars.saturating_add(chars);
        entry.duration_ms = entry.duration_ms.saturating_add(duration_ms);
        while cache.len() > ACTIVITY_RETENTION_DAYS {
            let Some(oldest) = cache.keys().next().cloned() else {
                break;
            };
            cache.remove(&oldest);
        }
        if let Some(path) = &self.path {
            let bytes = serde_json::to_vec_pretty(&*cache)
                .map_err(|_| persistence_error("encode activity aggregates"))?;
            atomic_write(path, &bytes)?;
        }
        Ok(())
    }

    pub fn snapshot(&self) -> Result<Vec<ActivityDay>, BackendError> {
        Ok(self
            .lock_cache()?
            .iter()
            .map(|(date, stats)| ActivityDay {
                date: date.clone(),
                count: stats.count,
                chars: stats.chars,
                duration_ms: stats.duration_ms,
            })
            .collect())
    }

    fn lock_cache(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, BTreeMap<String, DayStats>>, BackendError> {
        self.cache.lock().map_err(|_| {
            BackendError::new(BackendErrorCode::Internal, "activity store lock poisoned")
        })
    }
}

fn valid_date(date: &str) -> bool {
    let bytes = date.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
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
