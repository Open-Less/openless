//! An explicit Core Audio output tap, including on duplex audio interfaces.
use super::*;
use coreaudio_sys::AudioBufferList;
use std::ffi::c_void;
use std::time::Duration;

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
    reference: Arc<ReferenceAudioBuffer>,
    stop: Arc<AtomicBool>,
    diagnostics: Arc<CaptureDiagnostics>,
}

pub(in super::super) struct PlaybackReference {
    handle: *mut c_void,
    _context: Box<CallbackContext>,
}
impl Drop for PlaybackReference {
    fn drop(&mut self) {
        // Stop and destroy the IOProc before freeing its Rust callback context.
        unsafe {
            openless_playback_destroy(self.handle);
        }
    }
}

extern "C" fn callback(context: *mut c_void, input: *const AudioBufferList, age_ns: u64) {
    let context = unsafe { &*(context as *const CallbackContext) };
    if context.stop.load(Ordering::Acquire) || input.is_null() {
        return;
    }
    let captured_at = Instant::now()
        .checked_sub(Duration::from_nanos(age_ns))
        .unwrap_or_else(Instant::now);
    let input = unsafe { &*input };
    let buffers = unsafe {
        std::slice::from_raw_parts(input.mBuffers.as_ptr(), input.mNumberBuffers as usize)
    };
    let Some(first) = buffers.first().filter(|buffer| buffer.mNumberChannels > 0) else {
        return;
    };
    let frames = first.mDataByteSize as usize / 4 / first.mNumberChannels as usize;
    let channels = buffers
        .iter()
        .map(|buffer| buffer.mNumberChannels as usize)
        .sum::<usize>();
    if channels == 0 {
        return;
    }
    let mut mono = vec![0.0; frames];
    for buffer in buffers {
        let count = buffer.mNumberChannels as usize;
        if buffer.mData.is_null()
            || count == 0
            || buffer.mDataByteSize as usize != frames * count * 4
        {
            context.reference.invalidate();
            return;
        }
        let samples =
            unsafe { std::slice::from_raw_parts(buffer.mData.cast::<f32>(), frames * count) };
        for (output, frame) in mono.iter_mut().zip(samples.chunks_exact(count)) {
            *output += frame
                .iter()
                .map(|sample| {
                    if sample.is_finite() {
                        *sample / channels as f32
                    } else {
                        0.0
                    }
                })
                .sum::<f32>();
        }
    }
    context.diagnostics.data_callback(true, true, frames);
    context.reference.push_interleaved_at(&mono, 1, captured_at);
}

pub(in super::super) fn start(
    stop: &Arc<AtomicBool>,
    diagnostics: &Arc<CaptureDiagnostics>,
) -> Result<(PlaybackReference, Arc<ReferenceAudioBuffer>), RecorderError> {
    let mut rate = 0;
    let mut status = 0;
    let handle = unsafe { openless_playback_create(&mut rate, &mut status) };
    if handle.is_null() {
        return Err(RecorderError::EngineFailed(format!("system audio tap needs macOS 14.6+ and System Audio Recording permission (status {status})")));
    }
    let reference = Arc::new(ReferenceAudioBuffer::new(rate));
    let mut stream = PlaybackReference {
        handle,
        _context: Box::new(CallbackContext {
            reference: Arc::clone(&reference),
            stop: Arc::clone(stop),
            diagnostics: Arc::clone(diagnostics),
        }),
    };
    let status = unsafe {
        openless_playback_start(
            handle,
            callback,
            (&mut *stream._context as *mut CallbackContext).cast(),
        )
    };
    if status != 0 {
        return Err(RecorderError::EngineFailed(format!(
            "start system audio tap: {status}"
        )));
    }
    Ok((stream, reference))
}
