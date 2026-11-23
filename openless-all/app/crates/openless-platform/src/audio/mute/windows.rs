//! Windows：WASAPI 端点静音（IAudioEndpointVolume）。

use windows::Win32::Foundation::{BOOL, RPC_E_CHANGED_MODE};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
};

pub struct PlatformMuteGuard {
    was_muted: bool,
}

pub fn activate() -> Result<PlatformMuteGuard, String> {
    let was_muted = endpoint_muted()?;
    if !was_muted {
        set_endpoint_muted(true)?;
    }
    Ok(PlatformMuteGuard { was_muted })
}

impl PlatformMuteGuard {
    pub fn restore(self) {
        if let Err(err) = set_endpoint_muted(self.was_muted) {
            log::warn!("[audio-mute] restore endpoint mute failed: {err}");
        }
    }
}

fn endpoint_muted() -> Result<bool, String> {
    with_endpoint_volume(|endpoint| unsafe {
        let muted = endpoint
            .GetMute()
            .map_err(|e| format!("read endpoint mute state failed: {e}"))?;
        Ok(muted.as_bool())
    })
}

fn set_endpoint_muted(muted: bool) -> Result<(), String> {
    with_endpoint_volume(|endpoint| unsafe {
        endpoint
            .SetMute(BOOL(muted as i32), std::ptr::null())
            .map_err(|e| format!("set endpoint mute failed: {e}"))
    })
}

fn with_endpoint_volume<T>(
    f: impl FnOnce(&IAudioEndpointVolume) -> Result<T, String>,
) -> Result<T, String> {
    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let uninit_com = hr.is_ok();
        if !hr.is_ok() && hr != RPC_E_CHANGED_MODE {
            return Err(format!("CoInitializeEx failed: {hr:?}"));
        }

        let result = (|| {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .map_err(|e| format!("create audio device enumerator failed: {e}"))?;
            let device = enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .map_err(|e| format!("get default render endpoint failed: {e}"))?;
            let endpoint: IAudioEndpointVolume = device
                .Activate(CLSCTX_ALL, None)
                .map_err(|e| format!("activate endpoint volume failed: {e}"))?;
            f(&endpoint)
        })();

        if uninit_com {
            CoUninitialize();
        }
        result
    }
}
