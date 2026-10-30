//! Handles COM initialization and cleanup.

use std::marker::PhantomData;

use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};

thread_local!(static COM_INITIALIZED: Result<ComInitialized, windows::core::Error> = {
    unsafe {
        // Try to initialize COM with STA by default to avoid compatibility issues with the ASIO
        // backend (where CoInitialize() is called by the ASIO SDK) or winit (where drag and drop
        // requires STA).
        // This call can fail with RPC_E_CHANGED_MODE if another library initialized COM with MTA.
        // Reuse that apartment. This does not marshal any interface: callers must
        // keep every MMDevice interface in the thread that obtained it.
        let result = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        if result.is_ok() || result == RPC_E_CHANGED_MODE {
            Ok(ComInitialized {
                result,
                _ptr: PhantomData,
            })
        } else {
            Err(windows::core::Error::from_hresult(result))
        }
    }
});

/// RAII object that guards the fact that COM is initialized.
///
// We store a raw pointer because it's the only way at the moment to remove `Send`/`Sync` from the
// object.
struct ComInitialized {
    result: windows::core::HRESULT,
    _ptr: PhantomData<*mut ()>,
}

impl Drop for ComInitialized {
    #[inline]
    fn drop(&mut self) {
        // Need to avoid calling CoUninitialize() if CoInitializeEx failed since it may have
        // returned RPC_E_MODE_CHANGED - which is OK, see above.
        if self.result.is_ok() {
            unsafe { CoUninitialize() };
        }
    }
}

/// Ensures that COM is initialized in this thread.
#[inline]
pub fn com_initialized() {
    try_com_initialized().expect("cpal: COM initialization failed");
}

/// Fallible entry point for enumeration and the stream startup handshake.
/// TLS cleanup runs after the worker's stack (including every COM interface) is gone.
pub(super) fn try_com_initialized() -> Result<(), windows::core::Error> {
    COM_INITIALIZED.with(|result| result.as_ref().map(|_| ()).map_err(Clone::clone))
}
