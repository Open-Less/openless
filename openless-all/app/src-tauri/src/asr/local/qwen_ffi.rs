//! Minimal FFI declarations for the vendored Open-Less/qwen-asr public C API.
//!
//! Header: `vendor/qwen-asr/qwen_asr.h`. This does **not** replicate the internal layout of
//! `qwen_ctx_t` — an opaque pointer suffices, avoiding fragile pthread/alignment assumptions.

use std::os::raw::{c_char, c_int, c_void};

/// Opaque qwen_ctx_t; only ever passed around by pointer.
#[repr(C)]
pub struct QwenCtx {
    _opaque: [u8; 0],
}

/// `typedef void (*qwen_token_cb)(const char *piece, void *userdata);`
pub type QwenTokenCb = unsafe extern "C" fn(piece: *const c_char, userdata: *mut c_void);

// Keep the classic `extern "C"` block; call sites continue to carry the unsafe constraints.
extern "C" {
    pub fn qwen_load(model_dir: *const c_char) -> *mut QwenCtx;
    pub fn qwen_free(ctx: *mut QwenCtx);

    pub fn qwen_set_token_callback(
        ctx: *mut QwenCtx,
        cb: Option<QwenTokenCb>,
        userdata: *mut c_void,
    );
    pub fn qwen_set_prompt(ctx: *mut QwenCtx, prompt: *const c_char) -> c_int;
    pub fn qwen_set_force_language(ctx: *mut QwenCtx, language: *const c_char) -> c_int;
    pub fn qwen_supported_languages_csv() -> *const c_char;

    pub fn qwen_transcribe_audio(
        ctx: *mut QwenCtx,
        samples: *const f32,
        n_samples: c_int,
    ) -> *mut c_char;
    pub fn qwen_transcribe_stream(
        ctx: *mut QwenCtx,
        samples: *const f32,
        n_samples: c_int,
    ) -> *mut c_char;
}
