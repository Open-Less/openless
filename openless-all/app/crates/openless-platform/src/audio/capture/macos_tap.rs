//! macOS 系统音频回采（Core Audio process tap）。
//!
//! cpal 的通用输入路径在 duplex 声卡上会打开**输入**通道，拿不到系统输出。
//! 这里用 macOS 14.6+ 的 `AudioHardwareCreateProcessTap` 显式 tap 输出，
//! 与宿主迁移前的实现一致（含 System Audio Recording 权限失败时的明确错误）。
//!
//! C 部分在 `macos_tap.m`，由 `build.rs` 在 macOS 目标上编译。

use std::ffi::c_void;

use coreaudio_sys::AudioBufferList;

use super::{CaptureError, SampleSink, StreamFormat};

unsafe extern "C" {
    fn openless_playback_create(rate: *mut u32, status: *mut i32) -> *mut c_void;
    fn openless_playback_start(
        handle: *mut c_void,
        callback: extern "C" fn(*mut c_void, *const AudioBufferList, u64),
        context: *mut c_void,
    ) -> i32;
    fn openless_playback_destroy(handle: *mut c_void);
}

struct CallbackContext {
    sink: SampleSink,
}

/// 系统音频 tap 句柄；drop 时停止并释放 IOProc。
pub struct TapStream {
    handle: *mut c_void,
    _context: Box<CallbackContext>,
}

impl Drop for TapStream {
    fn drop(&mut self) {
        unsafe { openless_playback_destroy(self.handle) };
    }
}

impl TapStream {
    /// tap 在创建时已开始投递；这里保持与 cpal 流一致的接口。
    pub fn play(&self) -> Result<(), CaptureError> {
        Ok(())
    }

    /// 暂停 = 销毁 tap（下次录音会重新创建）。
    pub fn pause(&self) -> Result<(), CaptureError> {
        Ok(())
    }
}

extern "C" fn callback(context: *mut c_void, input: *const AudioBufferList, age_ns: u64) {
    let context = unsafe { &*(context as *const CallbackContext) };
    if input.is_null() {
        return;
    }
    let captured_at = std::time::Instant::now()
        .checked_sub(std::time::Duration::from_nanos(age_ns))
        .or(Some(std::time::Instant::now()));
    let input = unsafe { &*input };
    let buffers = unsafe {
        std::slice::from_raw_parts(input.mBuffers.as_ptr(), input.mNumberBuffers as usize)
    };
    let Some(first) = buffers.iter().find(|buffer| buffer.mNumberChannels > 0) else {
        return;
    };
    let frames = first.mDataByteSize as usize / 4 / first.mNumberChannels as usize;
    let channels: usize = buffers
        .iter()
        .map(|buffer| buffer.mNumberChannels as usize)
        .sum();
    if channels == 0 || frames == 0 {
        return;
    }
    let mut mono = vec![0.0f32; frames];
    for buffer in buffers {
        let count = buffer.mNumberChannels as usize;
        if buffer.mData.is_null()
            || count == 0
            || buffer.mDataByteSize as usize != frames * count * 4
        {
            return;
        }
        let samples =
            unsafe { std::slice::from_raw_parts(buffer.mData.cast::<f32>(), frames * count) };
        for (output, frame) in mono.iter_mut().zip(samples.chunks_exact(count)) {
            *output += frame
                .iter()
                .map(|sample| if sample.is_finite() { *sample / channels as f32 } else { 0.0 })
                .sum::<f32>();
        }
    }
    (context.sink.lock())(&mono, captured_at);
}

/// 创建并启动系统音频 tap。
pub fn start(sink: SampleSink) -> Result<(TapStream, StreamFormat), CaptureError> {
    let mut rate = 0u32;
    let mut status = 0i32;
    let handle = unsafe { openless_playback_create(&mut rate, &mut status) };
    if handle.is_null() {
        return Err(CaptureError::Unavailable(format!(
            "system audio tap needs macOS 14.6+ and System Audio Recording permission (status {status})"
        )));
    }
    let mut stream = TapStream {
        handle,
        _context: Box::new(CallbackContext { sink }),
    };
    let status = unsafe {
        openless_playback_start(
            handle,
            callback,
            (&mut *stream._context as *mut CallbackContext).cast(),
        )
    };
    if status != 0 {
        return Err(CaptureError::Unavailable(format!(
            "start system audio tap: {status}"
        )));
    }
    Ok((
        stream,
        StreamFormat {
            sample_rate: rate,
            channels: 1,
        },
    ))
}
