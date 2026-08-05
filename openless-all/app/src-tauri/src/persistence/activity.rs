//! 每日听写活动汇总（`date(YYYY-MM-DD) → {count, chars, duration_ms}`），供概览页的
//! 年度热力图与「近 7 天 / 近 30 天」统计使用。
//!
//! 与历史内容存储完全解耦：不含任何转写文本，也不受历史保留策略 / 条数上限影响
//! —— 清理历史不会抹掉活动足迹，热力图因此能覆盖全年而无需放开历史上限
//! （取代 PR #716 里「为热力图把历史改为无限保留」的方案）。
//! 写入时按保留窗口（两年）裁剪最早的日期，文件天然有界。
//!
//! 只存聚合数字、不存文本，所以「多记两个字段」的隐私与体积代价可忽略：一天一行，
//! 两年上限 731 行。

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Result;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use super::{atomic_write, data_dir, ensure_dir, read_or_default};

const ACTIVITY_FILE: &str = "activity.json";
/// 保留最近两年（含闰年余量）的日汇总，超窗的最早日期在写入时移除。
const ACTIVITY_RETENTION_DAYS: usize = 731;

/// 单日汇总。字段都是纯计数，不含任何文本。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DayStats {
    pub count: u32,
    #[serde(default)]
    pub chars: u64,
    #[serde(default)]
    pub duration_ms: u64,
}

/// 磁盘表示。旧版本的 activity.json 每天只写一个裸数字（`{"2026-08-01": 5}`），
/// 升级后必须原样读回来 —— 否则老用户的年度热力图会一次性清空。
/// 旧格式没有字数/时长，读回后为 0：这些天在新指标里显示为 0 是诚实的（数据当时没记），
/// 比整段丢掉条数要好。写入一律用新的对象格式。
#[derive(Deserialize)]
#[serde(untagged)]
enum StoredDay {
    /// 旧格式：只有条数。
    CountOnly(u32),
    /// 新格式。
    Full(DayStats),
}

impl From<StoredDay> for DayStats {
    fn from(stored: StoredDay) -> Self {
        match stored {
            StoredDay::CountOnly(count) => DayStats {
                count,
                ..Default::default()
            },
            StoredDay::Full(stats) => stats,
        }
    }
}

pub struct ActivityStore {
    path: PathBuf,
    cache: Mutex<BTreeMap<String, DayStats>>,
}

impl ActivityStore {
    pub fn load() -> Result<Self> {
        let dir = data_dir()?;
        ensure_dir(&dir)?;
        let path = dir.join(ACTIVITY_FILE);
        let stored: BTreeMap<String, StoredDay> = read_or_default(&path)?;
        let cache = stored
            .into_iter()
            .map(|(date, day)| (date, day.into()))
            .collect();
        Ok(Self {
            path,
            cache: Mutex::new(cache),
        })
    }

    /// load 失败时的内存降级：计数仍可累加（本次运行内有效），写盘静默失败。
    /// 活动计数是非关键路径，不因它阻断听写初始化。
    pub fn new_fallback() -> Self {
        Self {
            path: PathBuf::new(),
            cache: Mutex::new(BTreeMap::new()),
        }
    }

    /// 记录一次活动。`date` 为本地日期 `YYYY-MM-DD`（BTreeMap 按字典序即按日期序）。
    /// `chars` = 本次最终插入文本的字符数，`duration_ms` = 本次录音时长。
    /// 累加用 saturating：单日理论上不可能溢出，但计数器溢出 panic 不值得赌。
    pub fn bump(&self, date: &str, chars: u64, duration_ms: u64) -> Result<()> {
        let mut cache = self.cache.lock();
        let entry = cache.entry(date.to_string()).or_default();
        entry.count = entry.count.saturating_add(1);
        entry.chars = entry.chars.saturating_add(chars);
        entry.duration_ms = entry.duration_ms.saturating_add(duration_ms);
        while cache.len() > ACTIVITY_RETENTION_DAYS {
            let oldest = match cache.keys().next() {
                Some(key) => key.clone(),
                None => break,
            };
            cache.remove(&oldest);
        }
        let bytes = serde_json::to_vec_pretty(&*cache)?;
        atomic_write(&self.path, &bytes)
    }

    /// 全量快照（日期升序），前端聚合成热力图与周期指标。
    pub fn snapshot(&self) -> Vec<(String, DayStats)> {
        self.cache
            .lock()
            .iter()
            .map(|(date, stats)| (date.clone(), *stats))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{DayStats, StoredDay};
    use std::collections::BTreeMap;

    /// 老用户升级后 activity.json 仍是「日期 → 裸数字」。必须原样读回条数，
    /// 否则年度热力图一次性清空（用户会当成数据丢失）。
    #[test]
    fn legacy_count_only_entries_survive_the_upgrade() {
        let json = br#"{"2026-08-01": 5, "2026-08-02": 12}"#;
        let stored: BTreeMap<String, StoredDay> = serde_json::from_slice(json).unwrap();
        let parsed: BTreeMap<String, DayStats> =
            stored.into_iter().map(|(k, v)| (k, v.into())).collect();

        assert_eq!(parsed["2026-08-01"].count, 5);
        assert_eq!(parsed["2026-08-02"].count, 12);
        // 旧格式没记过字数/时长，读回 0 —— 诚实缺省，好过整天丢掉。
        assert_eq!(parsed["2026-08-01"].chars, 0);
        assert_eq!(parsed["2026-08-01"].duration_ms, 0);
    }

    #[test]
    fn new_object_entries_round_trip() {
        let original: BTreeMap<String, DayStats> = BTreeMap::from([(
            "2026-08-03".to_string(),
            DayStats {
                count: 7,
                chars: 4210,
                duration_ms: 96_000,
            },
        )]);
        let bytes = serde_json::to_vec(&original).unwrap();
        let stored: BTreeMap<String, StoredDay> = serde_json::from_slice(&bytes).unwrap();
        let parsed: BTreeMap<String, DayStats> =
            stored.into_iter().map(|(k, v)| (k, v.into())).collect();

        assert_eq!(parsed, original);
    }

    /// 两种格式混在同一个文件里也要能读：升级当天写入会把当天变成对象格式，
    /// 而更早的日期仍是裸数字。
    #[test]
    fn mixed_legacy_and_new_entries_parse_together() {
        let json = br#"{"2026-08-01": 5, "2026-08-02": {"count": 3, "chars": 900, "durationMs": 12000}}"#;
        let stored: BTreeMap<String, StoredDay> = serde_json::from_slice(json).unwrap();
        let parsed: BTreeMap<String, DayStats> =
            stored.into_iter().map(|(k, v)| (k, v.into())).collect();

        assert_eq!(parsed["2026-08-01"].count, 5);
        assert_eq!(parsed["2026-08-01"].chars, 0);
        assert_eq!(parsed["2026-08-02"].count, 3);
        assert_eq!(parsed["2026-08-02"].chars, 900);
        assert_eq!(parsed["2026-08-02"].duration_ms, 12_000);
    }

    /// 缺字段的对象（比如手工编辑过的文件）按 0 补齐，不整份读失败。
    #[test]
    fn object_entries_tolerate_missing_optional_fields() {
        let json = br#"{"2026-08-04": {"count": 2}}"#;
        let stored: BTreeMap<String, StoredDay> = serde_json::from_slice(json).unwrap();
        let parsed: BTreeMap<String, DayStats> =
            stored.into_iter().map(|(k, v)| (k, v.into())).collect();

        assert_eq!(
            parsed["2026-08-04"],
            DayStats {
                count: 2,
                chars: 0,
                duration_ms: 0
            }
        );
    }
}
