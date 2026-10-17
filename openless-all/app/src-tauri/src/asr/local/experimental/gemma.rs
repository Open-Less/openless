use super::*;
use std::ffi::{c_char, c_void, CStr, CString};
unsafe extern "C" {
    fn openless_gemma_load(
        path: *const c_char,
        checkpoint_context: *mut c_void,
        checkpoint: extern "C" fn(*mut c_void) -> bool,
        error: *mut *mut c_char,
    ) -> *mut c_void;
    fn openless_gemma_transcribe(
        handle: *mut c_void,
        samples: *const f32,
        count: usize,
        checkpoint_context: *mut c_void,
        checkpoint: extern "C" fn(*mut c_void) -> bool,
        error: *mut *mut c_char,
    ) -> *mut c_char;
    fn openless_gemma_release(handle: *mut c_void);
}
pub struct Handle(*mut c_void);
// Always accessed under the runtime slot mutex, including deallocation.
unsafe impl Send for Handle {}
unsafe fn take_string(value: *mut c_char) -> String {
    if value.is_null() {
        return "Unknown Core ML error".into();
    }
    let text = CStr::from_ptr(value).to_string_lossy().into_owned();
    libc::free(value.cast());
    text
}
impl Handle {
    pub fn load(
        path: &std::path::Path,
        budget: compute_policy::Budget,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, BackendError> {
        let mut checkpoint = Checkpoint {
            pacer: compute_policy::Pacer::new(budget),
            cancelled,
        };
        let path = CString::new(path.to_string_lossy().as_bytes()).map_err(failure)?;
        let mut error = std::ptr::null_mut();
        // SAFETY: Swift invokes the callback synchronously on this worker and
        // does not retain the stack context beyond model initialization.
        let handle = unsafe {
            openless_gemma_load(
                path.as_ptr(),
                (&mut checkpoint as *mut Checkpoint<'_>).cast(),
                checkpoint_cpu,
                &mut error,
            )
        };
        if handle.is_null() {
            return Err(failure(unsafe { take_string(error) }));
        }
        Ok(Self(handle))
    }
    pub fn transcribe(
        &mut self,
        samples: &[f32],
        budget: compute_policy::Budget,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<String, BackendError> {
        let mut checkpoint = Checkpoint {
            pacer: compute_policy::Pacer::new(budget),
            cancelled,
        };
        let mut error = std::ptr::null_mut();
        let text = unsafe {
            // SAFETY: Swift invokes the callback synchronously on this thread,
            // never retains its context, and returns before this stack state drops.
            openless_gemma_transcribe(
                self.0,
                samples.as_ptr(),
                samples.len(),
                (&mut checkpoint as *mut Checkpoint<'_>).cast(),
                checkpoint_cpu,
                &mut error,
            )
        };
        if text.is_null() {
            return Err(failure(unsafe { take_string(error) }));
        }
        Ok(unsafe { take_string(text) })
    }
}
struct Checkpoint<'a> {
    pacer: compute_policy::Pacer,
    cancelled: &'a dyn Fn() -> bool,
}
extern "C" fn checkpoint_cpu(context: *mut c_void) -> bool {
    // SAFETY: Handle::transcribe supplies a live, exclusively borrowed stack
    // context; Swift calls only on the same thread, within the enclosing FFI call.
    let checkpoint = unsafe { &*(context as *const Checkpoint<'_>) };
    checkpoint.pacer.checkpoint(checkpoint.cancelled)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn swift_load_honors_rust_cancellation_before_opening_model_files() {
        if ExperimentalNativeRuntime::unavailable(Model::EmbeddingGemma).is_some() {
            return;
        }
        let result = Handle::load(
            std::path::Path::new("/not-an-openless-model"),
            compute_policy::Budget::capture(),
            &|| true,
        );
        match result {
            Err(error) => assert!(error.message.contains("cancelled"), "{error:?}"),
            Ok(_) => panic!("cancelled Core ML load must not create a model"),
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { openless_gemma_release(self.0) }
    }
}
