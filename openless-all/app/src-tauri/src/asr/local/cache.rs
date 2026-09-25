#![cfg_attr(
    target_os = "linux",
    allow(dead_code, unused_imports, unused_variables)
)]
//! 本地 Qwen3-ASR 引擎缓存。
//!
//! 用途：避免每次 dictation 都重加载 1.2GB+ 模型。引擎一次 load 后驻留在内存，
//! 跨多次会话复用；用户在设置里决定"说完话即释放" / "保持 N 秒后释放" /
//! "不释放"。
//!
//! 调度规则：每次会话结束后 spawn 一个 sleep+check 任务；任务在到点时检查
//! `last_used`——如果中间又被使用过则不释放，否则 drop 引擎让 OS 回收 RAM。

use std::path::Path;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use parking_lot::Mutex;

#[cfg(any(target_os = "macos", target_os = "linux"))]
use super::{LocalQwenEngine, QwenBackend};

pub struct LocalAsrCache {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    inner: Mutex<Option<CachedEngine>>,
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    load_generation: AtomicU64,
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    _phantom: (),
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
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
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            inner: Mutex::new(None),
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            load_generation: AtomicU64::new(0),
            #[cfg(not(any(target_os = "macos", target_os = "linux")))]
            _phantom: (),
        }
    }

    /// 取已缓存的同 id 引擎，没有就加载（**阻塞、可能数秒**——调用方应放
    /// `spawn_blocking`）。模型 id 不同则把旧的 drop 再加载新的。
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    pub fn get_or_load(
        &self,
        backend: QwenBackend,
        model_id: &str,
        model_dir: &Path,
    ) -> Result<Arc<LocalQwenEngine>> {
        self.get_or_load_for_lease(backend, model_id, model_dir, None)
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
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
        // 迟到 loader 不得覆盖新 cache。普通听写仍按冻结上下文使用自己的 Arc；
        // 激活操作则必须报失败，否则调用方会把已被替代的模型提交为当前模型。
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
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    pub(crate) fn claim_lease(&self, model_id: &str, generation: u64) {
        let mut slot = self.inner.lock();
        self.load_generation.fetch_add(1, Ordering::AcqRel);
        if let Some(cached) = slot.as_mut().filter(|cached| cached.model_id == model_id) {
            cached.activation_generation = Some(generation);
        }
    }

    /// Core 只释放自己激活的那一代；同 ID 的新实例或普通 preload 都不属于旧 lease。
    #[cfg(any(target_os = "macos", target_os = "linux"))]
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
        // 驱逐不取消仍持 Arc 的转写，与 finish_use 的实例级收尾保持一致。
        if taken.is_some() {
            drop(taken);
            pressure_relief();
        }
    }

    /// 标记最近使用时间——end_session 在调过 transcribe 之后调一下，
    /// 让 release 计时器从这一刻重新算。
    pub fn touch(&self) {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            if let Some(cached) = self.inner.lock().as_mut() {
                cached.last_used = Instant::now();
            }
        }
    }

    /// Session 只允许清理自己实际借出且未被新激活认领的引擎。新激活可能复用
    /// 同一个 Arc，仍需保留它的 owner；下一次普通 get_or_load 才撤销该保护。
    #[cfg(any(target_os = "macos", target_os = "linux"))]
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
    #[cfg(any(target_os = "macos", target_os = "linux"))]
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

    /// 如果空闲时长 ≥ threshold，释放引擎。返回是否真释放了。
    pub fn release_if_idle(&self, idle_threshold: Duration) -> bool {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
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

    /// 从 cache 立刻驱逐，但不终止仍持有引擎的并发会话。会话结束、取消或超时后的
    /// 自动清理走这里，避免一个会话误杀另一个共享 MLX worker 的在途转写。
    pub fn evict_now(&self) {
        self.release_now_inner(false);
    }

    /// 立刻释放（用户点"立即释放"、切走 provider、删模型时调）。
    pub fn release_now(&self) {
        self.release_now_inner(true);
    }

    fn release_now_inner(&self, abort_in_use: bool) {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
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
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        let _ = abort_in_use;
    }

    pub fn loaded_model_id(&self) -> Option<String> {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            return self
                .inner
                .lock()
                .as_ref()
                .filter(|cached| cached.engine.is_healthy())
                .map(|cached| cached.model_id.clone());
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        None
    }
}

#[cfg(target_os = "linux")]
fn pressure_relief() {}

/// drop MLX Qwen 引擎后调一次：让 macOS libmalloc 把 freelist 上的物理页归还内核。
/// 不调的话，encoder f32 weights 那 ~几百 MB 的 free 不会立刻反映到 RSS，活动监视器
/// 看起来"释放按钮没生效"。decoder bf16 走 mmap，munmap 时已立即生效，不依赖这个调用。
#[cfg(target_os = "macos")]
fn pressure_relief() {
    // SAFETY: 系统 API；NULL zone + goal=0 = 对所有 zone 尽量多地归还，无内存安全风险。
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
