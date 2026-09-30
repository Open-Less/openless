//! Safe Rust wrapper around the vendored Open-Less/qwen-asr.
//!
//! Manages the model context, batch/streaming transcription, and token
//! callbacks; native calls on the same context are serialized by run_lock, and
//! the context must not be reused before a cancelled worker returns.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::path::Path;
use std::ptr;
use std::sync::{Mutex, MutexGuard};

use anyhow::{Context, Result};

use super::qwen_ffi::{
    qwen_free, qwen_load, qwen_set_token_callback, qwen_transcribe_audio, qwen_transcribe_stream,
    QwenCtx,
};

/// An FnMut closure is a fat pointer and cannot be stuffed directly into
/// `*mut c_void`, so it is wrapped in a Box.
type TokenHandler = dyn FnMut(&str) + Send + 'static;
type TokenHandlerBox = Box<Box<TokenHandler>>;

pub struct QwenAsrEngine {
    ctx: *mut QwenCtx,
    /// One C context cannot transcribe concurrently, nor swap the token
    /// callback mid-transcription. On cancel the upper layer only drops the
    /// `spawn_blocking` JoinHandle; an already-running native worker keeps
    /// going until it returns. This lock guarantees it is not reused by the
    /// next session before then.
    run_lock: Mutex<()>,
    /// Owns the token callback; the C side receives a raw ptr derived from
    /// `&**handler`, valid as long as this Box lives. The Mutex prevents
    /// concurrent set.
    token_handler: Mutex<Option<TokenHandlerBox>>,
}

/// SAFETY: the pthread/buffer inside `qwen_ctx_t` is used by the C side only
/// within a single transcribe call; `run_lock` guarantees the same context is
/// never called concurrently from two Rust threads. Send/Sync hold under that
/// constraint.
unsafe impl Send for QwenAsrEngine {}
unsafe impl Sync for QwenAsrEngine {}

impl QwenAsrEngine {
    /// Loads from a model directory (must contain `config.json` /
    /// `model.safetensors*` / `vocab.json` / `merges.txt`; layout described by
    /// qwen-asr's `download_model.sh`).
    pub fn load(model_dir: &Path) -> Result<Self> {
        let dir_str = model_dir
            .to_str()
            .with_context(|| format!("model dir 不是合法 UTF-8: {model_dir:?}"))?;
        let c_dir = CString::new(dir_str).context("model dir 含 NUL 字节")?;

        // SAFETY: `c_dir` lives for the duration of the call; NULL return means
        // load failure.
        let ctx = unsafe { qwen_load(c_dir.as_ptr()) };
        if ctx.is_null() {
            anyhow::bail!("qwen_load 失败：{model_dir:?}");
        }

        Ok(Self {
            ctx,
            run_lock: Mutex::new(()),
            token_handler: Mutex::new(None),
        })
    }

    /// Registers the streaming token callback; `None` clears it.
    /// Re-registering unbinds the old one before installing the new callback.
    pub fn set_token_handler<F>(&self, handler: Option<F>)
    where
        F: FnMut(&str) + Send + 'static,
    {
        let _run_guard = self.lock_run();
        self.set_token_handler_locked(handler);
    }

    fn lock_run(&self) -> MutexGuard<'_, ()> {
        self.run_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn set_token_handler_locked<F>(&self, handler: Option<F>)
    where
        F: FnMut(&str) + Send + 'static,
    {
        // std::sync::Mutex (not parking_lot) because this lock may be accessed
        // indirectly by the handler inside the C FFI callback token_trampoline;
        // if the handler panics, a std Mutex poisons. Recover via into_inner()
        // instead of panicking to avoid crashing the process.
        let mut slot = self
            .token_handler
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        // Clear the C side first, then drop the old Box, so C never holds the
        // old pointer across the swap.
        unsafe { qwen_set_token_callback(self.ctx, None, ptr::null_mut()) };
        *slot = None;

        if let Some(f) = handler {
            let boxed: TokenHandlerBox = Box::new(Box::new(f));
            // The inner `Box<TokenHandler>` of boxed has a stable heap address;
            // take its &mut and convert to raw.
            let userdata = boxed.as_ref() as *const Box<TokenHandler> as *mut c_void;
            unsafe {
                qwen_set_token_callback(self.ctx, Some(token_trampoline), userdata);
            }
            *slot = Some(boxed);
        }
    }

    /// Batch transcription: full audio (mono f32 16kHz) in one call.
    pub fn transcribe_audio(&self, samples: &[f32]) -> Result<String> {
        let _run_guard = self.lock_run();
        self.transcribe_audio_locked(samples)
    }

    fn transcribe_audio_locked(&self, samples: &[f32]) -> Result<String> {
        if self.ctx.is_null() {
            anyhow::bail!("engine already freed — cannot transcribe");
        }
        // SAFETY: samples live for the duration of the call; the return is a
        // C `malloc`ed string.
        let raw =
            unsafe { qwen_transcribe_audio(self.ctx, samples.as_ptr(), samples.len() as i32) };
        if raw.is_null() {
            anyhow::bail!("qwen_transcribe_audio 返回 NULL");
        }
        let text = unsafe { CStr::from_ptr(raw) }
            .to_string_lossy()
            .into_owned();
        unsafe { libc::free(raw as *mut c_void) };
        Ok(text)
    }

    /// Streaming transcription: internally slices into 2s chunks; tokens are
    /// emitted in real time through the callback registered via
    /// `set_token_handler`; the return value is the final complete text.
    pub fn transcribe_stream(&self, samples: &[f32]) -> Result<String> {
        let _run_guard = self.lock_run();
        self.transcribe_stream_locked(samples)
    }

    fn transcribe_stream_locked(&self, samples: &[f32]) -> Result<String> {
        if self.ctx.is_null() {
            anyhow::bail!("engine already freed — cannot transcribe");
        }
        let raw =
            unsafe { qwen_transcribe_stream(self.ctx, samples.as_ptr(), samples.len() as i32) };
        if raw.is_null() {
            anyhow::bail!("qwen_transcribe_stream 返回 NULL");
        }
        let text = unsafe { CStr::from_ptr(raw) }
            .to_string_lossy()
            .into_owned();
        unsafe { libc::free(raw as *mut c_void) };
        Ok(text)
    }

    /// Installs the callback, runs the native transcription, and unbinds the
    /// callback under the same context lock.
    ///
    /// When a `spawn_blocking` JoinHandle is cancelled, an already-started
    /// blocking closure may still run; unbinding the callback in the closure's
    /// own synchronous teardown path prevents an old session from holding or
    /// calling stale userdata during the next session.
    pub fn transcribe_stream_with_handler<F>(&self, samples: &[f32], handler: F) -> Result<String>
    where
        F: FnMut(&str) + Send + 'static,
    {
        let _run_guard = self.lock_run();
        self.set_token_handler_locked(Some(handler));
        let result = self.transcribe_stream_locked(samples);
        self.set_token_handler_locked::<fn(&str)>(None);
        result
    }
}

impl Drop for QwenAsrEngine {
    fn drop(&mut self) {
        if !self.ctx.is_null() {
            // Unbind the callback first so C never holds the userdata pointer
            // after free.
            unsafe {
                qwen_set_token_callback(self.ctx, None, ptr::null_mut());
                qwen_free(self.ctx);
            }
            self.ctx = ptr::null_mut();
        }
        // The token_handler Box is released when the Mutex is destructed.
    }
}

/// C trampoline: unwraps `userdata` back to `&mut Box<TokenHandler>` and
/// forwards the string.
unsafe extern "C" fn token_trampoline(piece: *const c_char, userdata: *mut c_void) {
    if userdata.is_null() || piece.is_null() {
        return;
    }
    // SAFETY: userdata is the `*Box<TokenHandler>` registered by
    // set_token_handler.
    let handler: &mut Box<TokenHandler> = unsafe { &mut *(userdata as *mut Box<TokenHandler>) };
    let text = unsafe { CStr::from_ptr(piece) }.to_string_lossy();
    handler(&text);
}
