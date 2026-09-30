//! 本地 Qwen3-ASR 引擎缓存。
//!
//! Purpose: avoid reloading the 1.2GB+ model on every dictation. Once loaded the engine stays
//! in memory and is reused across sessions; the user chooses in settings between
//! "release after speech" / "release after N seconds" / "never release".
//!
//! Scheduling rule: after each session a sleep+check task is spawned; when it fires it checks
//! `last_used` — if the engine was used meanwhile it is not released, otherwise it is dropped
//! so the OS reclaims the RAM.

use std::path::Path;
#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use parking_lot::Mutex;

#[cfg(target_os = "macos")]
use super::{LocalQwenEngine, QwenBackend};

pub struct LocalAsrCache {
    #[cfg(target_os = "macos")]
    inner: Mutex<Option<CachedEngine>>,
    #[cfg(target_os = "macos")]
    load_generation: AtomicU64,
    #[cfg(not(target_os = "macos"))]
    _phantom: (),
}

#[cfg(target_os = "macos")]
struct CachedEngine {
    model_id: String,
    backend: QwenBackend,
    engine: Arc<LocalQwenEngine>,
    last_used: Instant,
    activation_generation: Option<u64>,
}

impl Default for LocalAsrCache {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalAsrCache {
    pub fn new() -> Self {
        Self {
            #[cfg(target_os = "macos")]
            inner: Mutex::new(None),
            #[cfg(target_os = "macos")]
            load_generation: AtomicU64::new(0),
            #[cfg(not(target_os = "macos"))]
            _phantom: (),
        }
    }

    /// 取已缓存的同 id 引擎，没有就加载（**阻塞、可能数秒**——调用方应放
    /// `spawn_blocking`）。模型 id 不同则把旧的 drop 再加载新的。
    #[cfg(target_os = "macos")]
    pub fn get_or_load(
        &self,
        backend: QwenBackend,
        model_id: &str,
        model_dir: &Path,
    ) -> Result<Arc<LocalQwenEngine>> {
        self.get_or_load_for_lease(backend, model_id, model_dir, None)
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn get_or_load_for_lease(
        &self,
        backend: QwenBackend,
        model_id: &str,
        model_dir: &Path,
        activation_generation: Option<u64>,
    ) -> Result<Arc<LocalQwenEngine>> {
        let load_generation = {
            let mut slot = self.inner.lock();
            let generation = self.load_generation.fetch_add(1, Ordering::AcqRel) + 1;
            if let Some(cached) = slot.as_mut() {
                let same_target = cached.model_id == model_id && cached.backend == backend;
                if same_target && cached.engine.is_healthy() {
                    cached.last_used = Instant::now();
                    cached.activation_generation = activation_generation;
                    log::info!("[local-asr cache] reuse engine: {model_id}");
                    return Ok(Arc::clone(&cached.engine));
                }
                if same_target {
                    log::warn!(
                        "[local-asr cache] cached engine {} is unhealthy, reload",
                        cached.model_id
                    );
                } else {
                    log::info!(
                        "[local-asr cache] active model changed {} -> {}, drop old",
                        cached.model_id,
                        model_id
                    );
                }
                slot.take();
            }
            generation
        };
        log::info!(
            "[local-asr cache] loading {}:{model_id} from {}",
            backend.cache_key(),
            model_dir.display()
        );
        let engine = Arc::new(LocalQwenEngine::load(backend, model_dir)?);
        let mut slot = self.inner.lock();
        // A late loader must not overwrite the new cache. Ordinary dictation keeps using its
        // own Arc from the frozen context; activation operations must fail here instead, or the
        // caller would commit a superseded model as the current model.
        if self.load_generation.load(Ordering::Acquire) != load_generation {
            if activation_generation.is_some() {
                anyhow::bail!("本地 Qwen3-ASR 加载已被更新的操作替代");
            }
            return Ok(engine);
        }
        *slot = Some(CachedEngine {
            model_id: model_id.to_string(),
            backend,
            engine: Arc::clone(&engine),
            last_used: Instant::now(),
            activation_generation,
        });
        log::info!("[local-asr cache] loaded {model_id}");
        Ok(engine)
    }

    /// 在激活新模型前认领原缓存，也使尚未完成的旧 loader 失去写回 cache 的资格。
    #[cfg(target_os = "macos")]
    pub(crate) fn claim_lease(&self, model_id: &str, generation: u64) {
        let mut slot = self.inner.lock();
        self.load_generation.fetch_add(1, Ordering::AcqRel);
        if let Some(cached) = slot.as_mut().filter(|cached| cached.model_id == model_id) {
            cached.activation_generation = Some(generation);
        }
    }

    /// Core 只释放自己激活的那一代；同 ID 的新实例或普通 preload 都不属于旧 lease。
    #[cfg(target_os = "macos")]
    pub(crate) fn release_lease(&self, model_id: &str, generation: u64) {
        let taken = {
            let mut slot = self.inner.lock();
            if slot.as_ref().is_some_and(|cached| {
                cached.model_id == model_id && cached.activation_generation == Some(generation)
            }) {
                self.load_generation.fetch_add(1, Ordering::AcqRel);
                slot.take()
            } else {
                None
            }
        };
        // Eviction does not cancel transcriptions still holding an Arc, matching finish_use's
        // instance-level finalization.
        if taken.is_some() {
            drop(taken);
            pressure_relief();
        }
    }

    /// Mark last-used time — end_session calls this after transcribe so the release timer
    /// restarts from this moment.
    pub fn touch(&self) {
        #[cfg(target_os = "macos")]
        {
            if let Some(cached) = self.inner.lock().as_mut() {
                cached.last_used = Instant::now();
            }
        }
    }

    /// Session 只允许清理自己实际借出且未被新激活认领的引擎。新激活可能复用
    /// 同一个 Arc，仍需保留它的 owner；下一次普通 get_or_load 才撤销该保护。
    #[cfg(target_os = "macos")]
    pub fn finish_use(&self, engine: &Arc<LocalQwenEngine>, discard: bool) {
        let mut slot = self.inner.lock();
        if slot.as_ref().is_some_and(|cached| {
            cached.activation_generation.is_none() && Arc::ptr_eq(&cached.engine, engine)
        }) {
            if discard {
                slot.take();
            } else if let Some(cached) = slot.as_mut() {
                cached.last_used = Instant::now();
            }
        }
    }

    /// Timer 只保留 Weak，用户“立即释放”后不会被旧定时器额外占用数分钟 RAM。
    #[cfg(target_os = "macos")]
    pub fn release_current_if_idle(
        &self,
        engine: &std::sync::Weak<LocalQwenEngine>,
        threshold: Duration,
    ) {
        let mut slot = self.inner.lock();
        if slot.as_ref().is_some_and(|cached| {
            cached.activation_generation.is_none()
                && std::sync::Weak::ptr_eq(&Arc::downgrade(&cached.engine), engine)
                && cached.last_used.elapsed() >= threshold
        }) {
            slot.take();
            pressure_relief();
        }
    }

    /// Release the engine if idle for >= threshold. Returns whether it was actually released.
    pub fn release_if_idle(&self, idle_threshold: Duration) -> bool {
        #[cfg(target_os = "macos")]
        {
            let taken = {
                let mut slot = self.inner.lock();
                match slot.as_ref() {
                    Some(c)
                        if c.activation_generation.is_none()
                            && c.last_used.elapsed() >= idle_threshold =>
                    {
                        log::info!(
                            "[local-asr cache] release engine {} after idle {:?}",
                            c.model_id,
                            c.last_used.elapsed()
                        );
                        slot.take()
                    }
                    _ => None,
                }
            };
            if let Some(cached) = taken {
                drop(cached);
                pressure_relief();
                return true;
            }
        }
        let _ = idle_threshold;
        false
    }

    /// Evict from the cache immediately, but do not terminate concurrent sessions still holding
    /// the engine. Automatic cleanup after a session ends, cancels, or times out goes through
    /// here, avoiding one session accidentally killing another's in-flight transcription on a
    /// shared MLX worker.
    pub fn evict_now(&self) {
        self.release_now_inner(false);
    }

    /// Release immediately (called when the user clicks "Release now", switches provider, or
    /// deletes a model).
    pub fn release_now(&self) {
        self.release_now_inner(true);
    }

    fn release_now_inner(&self, abort_in_use: bool) {
        #[cfg(target_os = "macos")]
        {
            let taken = {
                let mut slot = self.inner.lock();
                self.load_generation.fetch_add(1, Ordering::AcqRel);
                slot.take()
            };
            if let Some(cached) = taken {
                let action = if abort_in_use { "release" } else { "evict" };
                log::info!("[local-asr cache] {action} engine {}", cached.model_id,);
                if abort_in_use && Arc::strong_count(&cached.engine) > 1 {
                    cached.engine.cancel();
                }
                drop(cached);
                pressure_relief();
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = abort_in_use;
    }

    pub fn loaded_model_id(&self) -> Option<String> {
        #[cfg(target_os = "macos")]
        {
            return self
                .inner
                .lock()
                .as_ref()
                .filter(|cached| cached.engine.is_healthy())
                .map(|cached| cached.model_id.clone());
        }
        #[cfg(not(target_os = "macos"))]
        None
    }
}

#[cfg(not(target_os = "macos"))]
fn pressure_relief() {}

/// Call once after dropping the MLX Qwen engine: makes macOS libmalloc return the physical pages
/// on its freelist to the kernel. Without it, the ~hundreds of MB freed by the encoder f32
/// weights does not immediately show up in RSS, so Activity Monitor looks like the "release"
/// button did nothing. The decoder bf16 uses mmap and takes effect immediately at munmap; it
/// does not rely on this call.
#[cfg(target_os = "macos")]
fn pressure_relief() {
    // SAFETY: system API; NULL zone + goal=0 = return as much as possible from all zones, no
    // memory-safety risk.
    let freed = unsafe { malloc_zone_pressure_relief(std::ptr::null_mut(), 0) };
    log::info!(
        "[local-asr cache] malloc_zone_pressure_relief freed ~{} bytes",
        freed
    );
}

#[cfg(target_os = "macos")]
extern "C" {
    fn malloc_zone_pressure_relief(zone: *mut libc::c_void, goal: libc::size_t) -> libc::size_t;
}
