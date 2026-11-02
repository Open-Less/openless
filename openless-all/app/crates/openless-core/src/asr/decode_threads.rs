//! Thread budget for local (sherpa-onnx) offline decode.
//!
//! Offline decode is the dictation latency floor: the user waits for it after
//! releasing the hotkey. It therefore must not run single-threaded. The numbers
//! below were measured on a 12-core Windows box with `sense-voice-small-zh`
//! int8 and a 15.0 s utterance (7 reps, median wall time):
//!
//! | threads | median | speedup |
//! |---------|--------|---------|
//! | 1       | 2.265s | 1.00x   |
//! | 2       | 1.378s | 1.64x   |
//! | 3       | 1.026s | 2.21x   |
//! | 4       | 1.019s | 2.22x   |
//! | 6       | 1.275s | 1.78x   |
//!
//! Three is the knee; past four the ONNX worker pool oversubscribes and loses
//! time. The transcript was byte-identical at every thread count, so raising the
//! thread count is a pure latency win and cannot change recognition output.
//!
//! Scope: these numbers are for SenseVoice only. Whisper and Paraformer have not
//! been measured, so they keep whatever thread count their call sites already
//! choose rather than inheriting a number that was never benchmarked on them.
//!
//! Safety: sherpa-onnx recognizers are not reentrant. The host drives every
//! decode through a serialized gate (`run_serialized_decode`), so a larger
//! intra-op pool never means two concurrent decodes on one recognizer.

/// Upper bound on ONNX intra-op threads for SenseVoice offline decode.
pub const SENSE_VOICE_DECODE_MAX_THREADS: usize = 3;

/// ONNX intra-op threads to request for SenseVoice offline decode on a host
/// with `cores` logical CPUs.
///
/// Clamped to `[1, SENSE_VOICE_DECODE_MAX_THREADS]`: a single-core host must
/// still get a valid (if minimal) pool, and a many-core host must not
/// oversubscribe.
pub fn sense_voice_decode_threads_for(cores: usize) -> i32 {
    cores.clamp(1, SENSE_VOICE_DECODE_MAX_THREADS) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_core_host_still_gets_a_valid_pool() {
        assert_eq!(sense_voice_decode_threads_for(0), 1);
        assert_eq!(sense_voice_decode_threads_for(1), 1);
    }

    #[test]
    fn pool_grows_with_cores_up_to_the_knee() {
        assert_eq!(sense_voice_decode_threads_for(2), 2);
        assert_eq!(sense_voice_decode_threads_for(3), 3);
    }

    #[test]
    fn many_core_host_does_not_oversubscribe() {
        // 4 threads measured no faster than 3, and 6 measured slower.
        assert_eq!(sense_voice_decode_threads_for(4), 3);
        assert_eq!(sense_voice_decode_threads_for(12), 3);
        assert_eq!(sense_voice_decode_threads_for(usize::MAX), 3);
    }

    #[test]
    fn windows_is_not_pinned_to_a_single_worker() {
        // The regression this guards: SenseVoice offline decode used to be forced
        // to one ONNX worker on Windows, costing ~2.2x decode latency for every
        // user on the most common desktop platform.
        assert!(
            sense_voice_decode_threads_for(12) > 1,
            "SenseVoice offline decode must use more than one ONNX worker on a multi-core host"
        );
    }
}
