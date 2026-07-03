#![cfg_attr(target_os = "linux", allow(dead_code, unused_variables))]
//! Daily activity stats store: aggregated per-day count/chars/duration for the
//! annual activity heatmap, decoupled from raw history text.
//!
//! History pruning (count cap or retention days) discards raw text but keeps
//! the aggregated daily stat, so the heatmap always has a full year of data.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use super::{atomic_write, data_dir, ensure_dir, read_or_default};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyActivityStat {
    pub date: String,
    pub session_count: u32,
    pub total_chars: u64,
    pub total_duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ActivityStatsFile {
    days: HashMap<String, DailyActivityStat>,
}

impl Default for ActivityStatsFile {
    fn default() -> Self {
        Self {
            days: HashMap::new(),
        }
    }
}

pub struct ActivityStatsStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl ActivityStatsStore {
    pub fn new() -> Result<Self> {
        let dir = data_dir()?;
        ensure_dir(&dir)?;
        Ok(Self {
            path: dir.join("activity_stats.json"),
            lock: Mutex::new(()),
        })
    }

    /// 在 data_dir 不可用时构造一个降级实例。
    pub fn new_fallback() -> Self {
        Self {
            path: std::env::temp_dir().join("openless_activity_stats_fallback.json"),
            lock: Mutex::new(()),
        }
    }

    /// 累加某日的统计数据（次数+1，字数+chars，时长+duration_ms）。
    /// date 格式：YYYY-MM-DD。
    pub fn add_session(&self, date: &str, chars: u64, duration_ms: u64) -> Result<()> {
        let _guard = self.lock.lock();
        let mut file = self.read_locked()?;
        let entry = file
            .days
            .entry(date.to_string())
            .or_insert_with(|| DailyActivityStat {
                date: date.to_string(),
                session_count: 0,
                total_chars: 0,
                total_duration_ms: 0,
            });
        entry.session_count += 1;
        entry.total_chars += chars;
        entry.total_duration_ms += duration_ms;
        self.write_locked(&file)
    }

    /// 获取全部日活统计，按日期升序排列。
    pub fn list(&self) -> Result<Vec<DailyActivityStat>> {
        let _guard = self.lock.lock();
        let file = self.read_locked()?;
        let mut stats: Vec<_> = file.days.into_values().collect();
        stats.sort_by(|a, b| a.date.cmp(&b.date));
        Ok(stats)
    }

    fn read_locked(&self) -> Result<ActivityStatsFile> {
        read_or_default::<ActivityStatsFile>(&self.path)
    }

    fn write_locked(&self, file: &ActivityStatsFile) -> Result<()> {
        let json =
            serde_json::to_vec_pretty(file).context("encode activity stats failed")?;
        atomic_write(&self.path, &json)
    }
}
