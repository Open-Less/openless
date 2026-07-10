#![cfg_attr(target_os = "linux", allow(dead_code, unused_variables))]
//! Dictation history store: newest-first JSON list with retention + count caps.

use std::path::PathBuf;

use anyhow::{Context, Result};
use parking_lot::Mutex;

use super::{atomic_write, data_dir, ensure_dir, read_or_default, HISTORY_CAP};
use crate::types::DictationSession;

const HISTORY_FILE: &str = "history.json";

pub struct HistoryStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl HistoryStore {
    pub fn new() -> Result<Self> {
        let dir = data_dir()?;
        ensure_dir(&dir)?;
        Ok(Self {
            path: dir.join(HISTORY_FILE),
            lock: Mutex::new(()),
        })
    }

    /// 在 data_dir 不可用时构造一个降级实例（指向临时目录）。
    /// 该实例在运行期间读写会安静地失败或返回空，不会 panic，
    /// 也不会影响正常启动路径。
    pub(crate) fn new_fallback() -> Self {
        Self {
            path: std::env::temp_dir().join("openless_history_fallback.json"),
            lock: Mutex::new(()),
        }
    }

    pub fn list(&self) -> Result<Vec<DictationSession>> {
        let _guard = self.lock.lock();
        self.read_locked()
    }

    /// `retention_days == 0` 跟旧 append 行为一致（不按时间清理）。
    /// `> 0` 时在写入新条目后顺手把超过 N 天的会话裁掉，写入时就完成清理，
    /// 不需要后台轮询。最后再受条数上限约束：
    /// - `max_entries == None` → HISTORY_CAP (200)
    /// - `max_entries == Some(n)` → clamp 到 5..=HISTORY_CAP，避免用户填 0 / 极大值。
    pub fn append_with_retention(
        &self,
        session: DictationSession,
        retention_days: u32,
        max_entries: Option<u32>,
    ) -> Result<()> {
        let _guard = self.lock.lock();
        let mut sessions = self.read_locked()?;
        // Prepend so the newest session is at index 0, matching the Swift impl.
        sessions.insert(0, session);
        if retention_days > 0 {
            let cutoff = chrono::Utc::now() - chrono::Duration::days(i64::from(retention_days));
            sessions.retain(|s| {
                chrono::DateTime::parse_from_rfc3339(&s.created_at)
                    .map(|t| t.with_timezone(&chrono::Utc) >= cutoff)
                    // 解析失败时保守保留——避免错误的时间戳让用户丢历史。
                    .unwrap_or(true)
            });
        }
        let cap = max_entries
            .map(|n| (n as usize).clamp(5, HISTORY_CAP))
            .unwrap_or(HISTORY_CAP);
        if sessions.len() > cap {
            sessions.truncate(cap);
        }
        self.write_locked(&sessions)
    }

    /// 返回最近 N 分钟内的会话（newest-first）。`minutes == 0` → 空 Vec，
    /// 调用方据此跳过对话感知 polish 路径。
    pub fn recent_within_minutes(&self, minutes: u32) -> Result<Vec<DictationSession>> {
        if minutes == 0 {
            return Ok(Vec::new());
        }
        let _guard = self.lock.lock();
        let sessions = self.read_locked()?;
        let cutoff = chrono::Utc::now() - chrono::Duration::minutes(i64::from(minutes));
        // sessions 是 newest-first，超出窗口的会话之后的都更老，take_while 即可。
        // unwrap_or(true)：时间戳解析失败时保留该条目，与 append_with_retention 的保守策略一致；
        // 避免单条坏记录截断整个上下文窗口。
        let filtered: Vec<DictationSession> = sessions
            .into_iter()
            .take_while(|s| {
                chrono::DateTime::parse_from_rfc3339(&s.created_at)
                    .map(|t| t.with_timezone(&chrono::Utc) >= cutoff)
                    .unwrap_or(true)
            })
            .collect();
        Ok(filtered)
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let _guard = self.lock.lock();
        let mut sessions = self.read_locked()?;
        let original_len = sessions.len();
        sessions.retain(|s| s.id != id);
        if sessions.len() == original_len {
            return Ok(());
        }
        self.write_locked(&sessions)
    }

    pub fn update_entry(&self, updated: DictationSession) -> Result<bool> {
        let _guard = self.lock.lock();
        let mut sessions = self.read_locked()?;
        let Some(slot) = sessions.iter_mut().find(|s| s.id == updated.id) else {
            return Ok(false);
        };
        *slot = updated;
        self.write_locked(&sessions)?;
        Ok(true)
    }

    pub fn clear(&self) -> Result<()> {
        let _guard = self.lock.lock();
        self.write_locked(&Vec::<DictationSession>::new())
    }

    fn read_locked(&self) -> Result<Vec<DictationSession>> {
        read_or_default::<Vec<DictationSession>>(&self.path)
    }

    fn write_locked(&self, sessions: &[DictationSession]) -> Result<()> {
        let json = serde_json::to_vec_pretty(sessions).context("encode history failed")?;
        atomic_write(&self.path, &json)
    }
}
