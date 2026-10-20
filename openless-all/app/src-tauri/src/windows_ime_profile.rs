#![allow(dead_code, unused_imports, unused_variables)]
/// All languages (`0xFFFF`). The text service is registered under the TSF speech
/// category, which TSF keeps active next to the user's keyboard IME, so dictation
/// never switches the keyboard IME away and back.
pub const OPENLESS_TSF_LANG_ID: u16 = 0xFFFF;
pub const OPENLESS_TEXT_SERVICE_CLSID_BRACED: &str = "{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}";
pub const OPENLESS_PROFILE_GUID_BRACED: &str = "{9B5F5E04-23F6-47DA-9A26-D221F6C3F02E}";

use crate::types::{WindowsImeInstallState, WindowsImeStatus};

#[cfg(target_os = "windows")]
fn parse_guid(value: &str) -> WindowsImeProfileResult<windows::core::GUID> {
    uuid::Uuid::parse_str(value)
        .map(|uuid| windows::core::GUID::from_u128(uuid.as_u128()))
        .map_err(|err| WindowsImeProfileError::WindowsApi(format!("invalid GUID {value}: {err}")))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowsImeProfileError {
    Unavailable(String),
    WindowsApi(String),
}

impl std::fmt::Display for WindowsImeProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(message) | Self::WindowsApi(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for WindowsImeProfileError {}

pub type WindowsImeProfileResult<T> = Result<T, WindowsImeProfileError>;

pub fn get_windows_ime_status() -> WindowsImeStatus {
    #[cfg(target_os = "windows")]
    {
        windows_impl::get_windows_ime_status()
    }

    #[cfg(not(target_os = "windows"))]
    {
        WindowsImeStatus {
            state: WindowsImeInstallState::NotWindows,
            using_tsf_backend: false,
            message: "Windows TSF IME backend is only available on Windows".to_string(),
            dll_path: None,
        }
    }
}

#[cfg(target_os = "windows")]
pub fn set_openless_language_profile_enabled(enabled: bool) -> WindowsImeProfileResult<()> {
    windows_impl::set_openless_language_profile_enabled(enabled)
}

#[cfg(not(target_os = "windows"))]
pub fn set_openless_language_profile_enabled(_enabled: bool) -> WindowsImeProfileResult<()> {
    Err(WindowsImeProfileError::Unavailable(
        "Windows TSF profiles are only available on Windows".to_string(),
    ))
}

#[cfg(target_os = "windows")]
pub fn is_openless_language_profile_enabled() -> WindowsImeProfileResult<bool> {
    windows_impl::is_openless_language_profile_enabled()
}

#[cfg(not(target_os = "windows"))]
pub fn is_openless_language_profile_enabled() -> WindowsImeProfileResult<bool> {
    Err(WindowsImeProfileError::Unavailable(
        "Windows TSF profiles are only available on Windows".to_string(),
    ))
}

/// Short-circuit result for the "keyboard list visibility" preference when the TSF IME is not
/// installed (or its registration is broken).
///
/// Returning `Some(result)` ends the operation without touching the registry; `None` means the
/// IME is installed and the real `EnableLanguageProfile` change must run. Pure function so the
/// "not installed" branch is testable on any platform (`apply_windows_openless_keyboard_list`
/// depends on the Windows registry; macOS/CI can't reach its internal branches).
///
/// Both branches must be `Ok(())`: with the TSF IME not installed there is nothing to enable or
/// hide. Returning `Err` here propagated through the settings.rs transaction and rolled back the
/// whole settings save.
fn keyboard_list_pref_short_circuit(
    install_state: WindowsImeInstallState,
    _desired: bool,
) -> Option<Result<(), String>> {
    if install_state == WindowsImeInstallState::Installed {
        None
    } else {
        Some(Ok(()))
    }
}

/// Keep the current user's TSF language profile enabled.
///
/// The text service is a speech-category profile: it never appears in the keyboard
/// list, and TSF only activates it while the profile is enabled. The "show in
/// keyboard list" preference therefore no longer disables it; `desired` is kept
/// so the not-installed short-circuit and the callers stay unchanged.
pub fn apply_windows_openless_keyboard_list(desired: bool) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let status = get_windows_ime_status();
        if let Some(result) = keyboard_list_pref_short_circuit(status.state, desired) {
            return result;
        }
        set_openless_language_profile_enabled(true).map_err(|err| {
            let message = err.to_string();
            log::warn!("[windows-ime] enabling the TSF language profile failed: {message}");
            message
        })
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = desired;
        Ok(())
    }
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use super::*;
    use std::path::Path;
    use windows::core::HRESULT;
    use windows::Win32::Foundation::BOOL;
    use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::TextServices::{
        CLSID_TF_InputProcessorProfiles, ITfInputProcessorProfiles,
    };
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY};
    use winreg::RegKey;

    const OPENLESS_COM_INPROC_KEY: &str =
        r"Software\Classes\CLSID\{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}\InprocServer32";
    const OPENLESS_TSF_PROFILE_KEY: &str = r"Software\Microsoft\CTF\TIP\{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}\LanguageProfile\0x0000ffff\{9B5F5E04-23F6-47DA-9A26-D221F6C3F02E}";
    const OPENLESS_TSF_SPEECH_CATEGORY_KEY: &str = r"Software\Microsoft\CTF\TIP\{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}\Category\Category\{B5A73CD1-8355-426B-A161-259808F26B14}\{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}";
    const OPENLESS_TSF_IMMERSIVE_CATEGORY_KEY: &str = r"Software\Microsoft\CTF\TIP\{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}\Category\Category\{13A016DF-560B-46CD-947A-4C3AF1E0E35D}\{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}";
    const OPENLESS_TSF_SYSTRAY_CATEGORY_KEY: &str = r"Software\Microsoft\CTF\TIP\{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}\Category\Category\{25504FB4-7BAB-4BC1-9C69-CF81890F0EF5}\{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}";

    pub(super) struct ComInitializeOwnership {
        pub(super) should_uninitialize: bool,
    }

    pub(super) fn coinitialize_result_ownership(
        result: HRESULT,
    ) -> WindowsImeProfileResult<ComInitializeOwnership> {
        if result == RPC_E_CHANGED_MODE {
            return Ok(ComInitializeOwnership {
                should_uninitialize: false,
            });
        }

        result
            .ok()
            .map(|_| ComInitializeOwnership {
                should_uninitialize: true,
            })
            .map_err(|err| WindowsImeProfileError::WindowsApi(format!("CoInitializeEx: {err}")))
    }

    struct ComApartment {
        should_uninitialize: bool,
    }

    impl ComApartment {
        fn initialize() -> WindowsImeProfileResult<Self> {
            let ownership = coinitialize_result_ownership(unsafe {
                CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            })?;
            Ok(Self {
                should_uninitialize: ownership.should_uninitialize,
            })
        }
    }

    impl Drop for ComApartment {
        fn drop(&mut self) {
            if !self.should_uninitialize {
                return;
            }
            unsafe {
                CoUninitialize();
            }
        }
    }

    pub fn set_openless_language_profile_enabled(enabled: bool) -> WindowsImeProfileResult<()> {
        let clsid = parse_guid(OPENLESS_TEXT_SERVICE_CLSID_BRACED)?;
        let profile_guid = parse_guid(OPENLESS_PROFILE_GUID_BRACED)?;
        let enable_flag = BOOL::from(enabled);

        with_input_processor_profiles(|profiles| unsafe {
            profiles.EnableLanguageProfile(&clsid, OPENLESS_TSF_LANG_ID, &profile_guid, enable_flag)
        })
    }

    pub fn is_openless_language_profile_enabled() -> WindowsImeProfileResult<bool> {
        let clsid = parse_guid(OPENLESS_TEXT_SERVICE_CLSID_BRACED)?;
        let profile_guid = parse_guid(OPENLESS_PROFILE_GUID_BRACED)?;

        with_input_processor_profiles(|profiles| unsafe {
            let enabled =
                profiles.IsEnabledLanguageProfile(&clsid, OPENLESS_TSF_LANG_ID, &profile_guid)?;
            Ok(enabled.as_bool())
        })
    }

    pub fn get_windows_ime_status() -> WindowsImeStatus {
        match inspect_windows_ime_registration() {
            RegistrationInspection::Installed { dll_path } => WindowsImeStatus {
                state: WindowsImeInstallState::Installed,
                using_tsf_backend: true,
                message: "OpenLess TSF IME registration is present".to_string(),
                dll_path: Some(dll_path),
            },
            RegistrationInspection::NotInstalled => WindowsImeStatus {
                state: WindowsImeInstallState::NotInstalled,
                using_tsf_backend: false,
                message: "OpenLess TSF IME registration was not found".to_string(),
                dll_path: None,
            },
            RegistrationInspection::Broken { dll_path, reason } => WindowsImeStatus {
                state: WindowsImeInstallState::RegistrationBroken,
                using_tsf_backend: false,
                message: reason,
                dll_path,
            },
        }
    }

    enum RegistrationInspection {
        Installed {
            dll_path: String,
        },
        NotInstalled,
        Broken {
            dll_path: Option<String>,
            reason: String,
        },
    }

    fn inspect_windows_ime_registration() -> RegistrationInspection {
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        let com_key =
            hklm.open_subkey_with_flags(OPENLESS_COM_INPROC_KEY, KEY_READ | KEY_WOW64_64KEY);
        let tip_key_exists = hklm
            .open_subkey_with_flags(OPENLESS_TSF_PROFILE_KEY, KEY_READ | KEY_WOW64_64KEY)
            .is_ok();
        let speech_category_exists = hklm
            .open_subkey_with_flags(OPENLESS_TSF_SPEECH_CATEGORY_KEY, KEY_READ | KEY_WOW64_64KEY)
            .is_ok();
        let immersive_category_exists = hklm
            .open_subkey_with_flags(
                OPENLESS_TSF_IMMERSIVE_CATEGORY_KEY,
                KEY_READ | KEY_WOW64_64KEY,
            )
            .is_ok();
        let systray_category_exists = hklm
            .open_subkey_with_flags(
                OPENLESS_TSF_SYSTRAY_CATEGORY_KEY,
                KEY_READ | KEY_WOW64_64KEY,
            )
            .is_ok();

        if com_key.is_err() && !tip_key_exists && !speech_category_exists {
            return RegistrationInspection::NotInstalled;
        }

        let com_key = match com_key {
            Ok(key) => key,
            Err(_) => {
                return RegistrationInspection::Broken {
                    dll_path: None,
                    reason: "OpenLess COM registration is missing".to_string(),
                };
            }
        };

        let dll_path: String = match com_key.get_value::<String, _>("") {
            Ok(value) if !value.trim().is_empty() => value,
            _ => {
                return RegistrationInspection::Broken {
                    dll_path: None,
                    reason: "OpenLess COM DLL path is missing".to_string(),
                };
            }
        };

        if !Path::new(&dll_path).is_file() {
            return RegistrationInspection::Broken {
                dll_path: Some(dll_path),
                reason: "OpenLess COM DLL path does not exist".to_string(),
            };
        }

        let x86_dll_path = match read_com_dll_path(&hklm, KEY_READ | KEY_WOW64_32KEY, "32-bit") {
            Ok(path) => path,
            Err(reason) => {
                return RegistrationInspection::Broken {
                    dll_path: Some(dll_path),
                    reason,
                };
            }
        };
        if !Path::new(&x86_dll_path).is_file() {
            return RegistrationInspection::Broken {
                dll_path: Some(x86_dll_path),
                reason: "OpenLess 32-bit COM DLL path does not exist".to_string(),
            };
        }

        if !tip_key_exists {
            return RegistrationInspection::Broken {
                dll_path: Some(dll_path),
                reason: "OpenLess TSF language profile registration is missing".to_string(),
            };
        }

        if !speech_category_exists {
            // Also the state right after upgrading the app while a DLL from a
            // release that registered a keyboard profile is still in place.
            return RegistrationInspection::Broken {
                dll_path: Some(dll_path),
                reason: "OpenLess TSF speech category registration is missing; reinstall the IME"
                    .to_string(),
            };
        }

        if !immersive_category_exists || !systray_category_exists {
            return RegistrationInspection::Broken {
                dll_path: Some(dll_path),
                reason: "OpenLess TSF immersive support registration is missing; reinstall the IME"
                    .to_string(),
            };
        }

        RegistrationInspection::Installed { dll_path }
    }

    fn read_com_dll_path(hklm: &RegKey, flags: u32, label: &str) -> Result<String, String> {
        let com_key = hklm
            .open_subkey_with_flags(OPENLESS_COM_INPROC_KEY, flags)
            .map_err(|_| format!("OpenLess {label} COM registration is missing"))?;
        match com_key.get_value::<String, _>("") {
            Ok(value) if !value.trim().is_empty() => Ok(value),
            _ => Err(format!("OpenLess {label} COM DLL path is missing")),
        }
    }

    fn with_input_processor_profiles<T>(
        operation: impl FnOnce(&ITfInputProcessorProfiles) -> windows::core::Result<T>,
    ) -> WindowsImeProfileResult<T> {
        let _com = ComApartment::initialize()?;
        let profiles: ITfInputProcessorProfiles = unsafe {
            CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER)
        }
        .map_err(windows_api_error(
            "CoCreateInstance ITfInputProcessorProfiles",
        ))?;

        operation(&profiles).map_err(windows_api_error("ITfInputProcessorProfiles operation"))
    }

    fn windows_api_error(
        context: &'static str,
    ) -> impl FnOnce(windows::core::Error) -> WindowsImeProfileError {
        move |err| WindowsImeProfileError::WindowsApi(format!("{context}: {err}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── The "keyboard list visibility" preference must not error when the TSF IME is not installed ──
    // A not-installed state previously returned Err, which propagated through settings.rs's
    // apply_keyboard_list(&prefs)? and rolled back the whole settings save transaction.
    // Regression guard.

    #[test]
    fn uninstalled_hide_request_is_noop_ok() {
        assert_eq!(
            keyboard_list_pref_short_circuit(WindowsImeInstallState::NotInstalled, false),
            Some(Ok(()))
        );
    }

    #[test]
    fn uninstalled_show_request_is_noop_ok() {
        assert_eq!(
            keyboard_list_pref_short_circuit(WindowsImeInstallState::NotInstalled, true),
            Some(Ok(()))
        );
    }

    #[test]
    fn broken_registration_short_circuits_ok_for_both_desired_values() {
        assert_eq!(
            keyboard_list_pref_short_circuit(WindowsImeInstallState::RegistrationBroken, false),
            Some(Ok(()))
        );
        assert_eq!(
            keyboard_list_pref_short_circuit(WindowsImeInstallState::RegistrationBroken, true),
            Some(Ok(()))
        );
    }

    #[test]
    fn not_windows_state_short_circuits_ok() {
        assert_eq!(
            keyboard_list_pref_short_circuit(WindowsImeInstallState::NotWindows, false),
            Some(Ok(()))
        );
    }

    #[test]
    fn installed_state_proceeds_to_real_profile_mutation() {
        assert_eq!(
            keyboard_list_pref_short_circuit(WindowsImeInstallState::Installed, false),
            None
        );
        assert_eq!(
            keyboard_list_pref_short_circuit(WindowsImeInstallState::Installed, true),
            None
        );
    }
}

#[cfg(all(test, target_os = "windows"))]
mod windows_tests {
    use super::*;
    use windows::Win32::Foundation::RPC_E_CHANGED_MODE;

    #[test]
    fn openless_profile_identifiers_are_fixed() {
        assert_eq!(OPENLESS_TSF_LANG_ID, 0xFFFF);
        assert_eq!(
            OPENLESS_TEXT_SERVICE_CLSID_BRACED,
            "{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}"
        );
        assert_eq!(
            OPENLESS_PROFILE_GUID_BRACED,
            "{9B5F5E04-23F6-47DA-9A26-D221F6C3F02E}"
        );
    }

    #[test]
    fn com_changed_mode_is_accepted_without_uninitializing() {
        let ownership = windows_impl::coinitialize_result_ownership(RPC_E_CHANGED_MODE).unwrap();

        assert!(!ownership.should_uninitialize);
    }
}
