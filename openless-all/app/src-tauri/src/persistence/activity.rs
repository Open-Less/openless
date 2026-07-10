//! 每日听写活动计数（`date(YYYY-MM-DD) → count`），供概览页年度活动热力图使用。
//!
//! 与历史内容存储完全解耦：不含任何转写文本，也不受历史保留策略 / 条数上限影响
//! —— 清理历史不会抹掉活动足迹，热力图因此能覆盖全年而无需放开历史上限
//! （取代 PR #716 里「为热力图把历史改为无限保留」的方案）。
//! 写入时按保留窗口（两年）裁剪最早的日期，文件天然有界。

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Result;
use parking_lot::Mutex;

use super::{atomic_write, data_dir, ensure_dir, read_or_default};

const ACTIVITY_FILE: &str = "activity.json";
/// 保留最近两年（含闰年余量）的日计数，超窗的最早日期在写入时移除。
const ACTIVITY_RETENTION_DAYS: usize = 731;

pub struct ActivityStore {
    path: PathBuf,
    cache: Mutex<BTreeMap<String, u32>>,
}

impl ActivityStore {
    pub fn load() -> Result<Self> {
        let dir = data_dir()?;
        ensure_dir(&dir)?;
        let path = dir.join(ACTIVITY_FILE);
        let cache: BTreeMap<String, u32> = read_or_default(&path)?;
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
    pub fn bump(&self, date: &str) -> Result<()> {
        let mut cache = self.cache.lock();
        *cache.entry(date.to_string()).or_insert(0) += 1;
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

    /// 全量快照（日期升序），前端聚合成热力图。
    pub fn snapshot(&self) -> Vec<(String, u32)> {
        self.cache
            .lock()
            .iter()
            .map(|(date, count)| (date.clone(), *count))
            .collect()
    }
}
