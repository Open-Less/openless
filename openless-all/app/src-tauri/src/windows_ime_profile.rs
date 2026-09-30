#![allow(dead_code, unused_imports, unused_variables)]
pub const OPENLESS_TSF_LANG_ID: u16 = 0x0804;
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
pub enum ImeProfileKind {
    KeyboardLayout,
    TextService,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImeProfileSnapshot {
    kind: ImeProfileKind,
    lang_id: u16,
    clsid: Option<String>,
    profile_guid: Option<String>,
    hkl: Option<isize>,
}

impl ImeProfileSnapshot {
    pub fn text_service(lang_id: u16, clsid: String, profile_guid: String) -> Self {
        Self {
            kind: ImeProfileKind::TextService,
            lang_id,
            clsid: Some(clsid),
            profile_guid: Some(profile_guid),
            hkl: None,
        }
    }

    pub fn keyboard_layout(lang_id: u16, hkl: isize) -> Self {
        Self {
            kind: ImeProfileKind::KeyboardLayout,
            lang_id,
            clsid: None,
            profile_guid: None,
            hkl: Some(hkl),
        }
    }

    pub fn kind(&self) -> &ImeProfileKind {
        &self.kind
    }

    pub fn lang_id(&self) -> u16 {
        self.lang_id
    }

    pub fn clsid(&self) -> Option<&str> {
        self.clsid.as_deref()
    }

    pub fn profile_guid(&self) -> Option<&str> {
        self.profile_guid.as_deref()
    }

    pub fn hkl(&self) -> Option<isize> {
        self.hkl
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileRestoreDecision {
    RestoreSavedProfile,
    KeepCurrentProfile,
}

/// Whether the snapshot is OpenLess's own TSF profile.
///
/// Sticky-state protection: if the last session's restore failed and OpenLess is still the
/// current IME, the next `prepare_session` would capture OpenLess itself as the "original
/// IME"; restore must then be skipped, otherwise OpenLess gets persisted as the original
/// IME (the self-sticky failure state of issue #852).
pub fn is_openless_profile_snapshot(snapshot: &ImeProfileSnapshot) -> bool {
    matches!(snapshot.kind(), ImeProfileKind::TextService)
        && snapshot.lang_id() == OPENLESS_TSF_LANG_ID
        && snapshot.clsid().map(normalize_guid_string).as_deref()
            == Some(OPENLESS_TEXT_SERVICE_CLSID_BRACED)
        && snapshot
            .profile_guid()
            .map(normalize_guid_string)
            .as_deref()
            == Some(OPENLESS_PROFILE_GUID_BRACED)
}

/// Test-only: build a TSF snapshot of OpenLess itself.
///
/// Identifiers derive from the production constants (lowercased to exercise the GUID
/// normalization path), so test literals cannot drift from production constants — if a
/// constant changes, the test follows and validates the new value.
#[cfg(test)]
pub(crate) fn openless_snapshot_for_test() -> ImeProfileSnapshot {
    ImeProfileSnapshot::text_service(
        OPENLESS_TSF_LANG_ID,
        OPENLESS_TEXT_SERVICE_CLSID_BRACED.to_ascii_lowercase(),
        OPENLESS_PROFILE_GUID_BRACED.to_ascii_lowercase(),
    )
}

fn normalize_guid_string(value: &str) -> String {
    let upper = value.trim().to_ascii_uppercase();
    if upper.starts_with('{') && upper.ends_with('}') {
        upper
    } else {
        format!("{{{upper}}}")
    }
}

/// Decide whether to restore the original IME based on session state.
///
/// - The session really activated OpenLess (`openless_was_activated`) → restore;
/// - Activation failed but the original snapshot was captured (`openless_activation_failed`)
///   → still restore, cleaning up a half-finished activation's leftover state;
/// - Neither activated nor a failure snapshot (original IME never captured / non-Windows)
///   → keep the current profile.
///
/// Note: this **no longer** takes the result of `is_openless_profile_active()`. That probe
/// runs on a background thread of OpenLess's own process, while OpenLess IME activation
/// happens in the target app's process; `GetActiveProfile` may return a thread-local default
/// profile, misjudged as "the user already switched away" and skipping the restore
/// (issue #852). The restore decision must rely only on activation facts we already know.
pub fn restore_decision(
    saved: Option<&ImeProfileSnapshot>,
    openless_was_activated: bool,
    openless_activation_failed: bool,
) -> ProfileRestoreDecision {
    if saved.is_some() && (openless_was_activated || openless_activation_failed) {
        ProfileRestoreDecision::RestoreSavedProfile
    } else {
        ProfileRestoreDecision::KeepCurrentProfile
    }
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
/// Key semantics: with the TSF IME not installed, the keyboard list has no OpenLess entry at all —
/// - `desired == false` (hide) is already satisfied, a natural no-op;
/// - `desired == true` (show) can only be a no-op too (nothing to enable).
///
/// Both branches must be `Ok(())`. Previously "hide + not installed" wrongly returned `Err`,
/// which propagated through the settings.rs transaction and rolled back the whole settings
/// save (with a non-TSF insertion method and the TSF IME not installed, checking "don't show
/// in the keyboard list" made any later setting impossible to save).
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

/// Sync the keyboard list target state already decided by Core into the current user's TSF language profile.
pub fn apply_windows_openless_keyboard_list(desired: bool) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let status = get_windows_ime_status();
        if let Some(result) = keyboard_list_pref_short_circuit(status.state, desired) {
            return result;
        }
        set_openless_language_profile_enabled(desired).map_err(|err| {
            let message = err.to_string();
            log::warn!("[windows-ime] apply keyboard list visibility pref failed: {message}");
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
pub struct WindowsImeProfileManager;

#[cfg(target_os = "windows")]
impl WindowsImeProfileManager {
    pub fn new() -> Self {
        Self
    }

    pub fn capture_active_profile(&self) -> WindowsImeProfileResult<ImeProfileSnapshot> {
        windows_impl::capture_active_profile()
    }

    pub fn activate_openless_profile(&self) -> WindowsImeProfileResult<()> {
        windows_impl::activate_openless_profile()
    }

    pub fn restore_profile(&self, snapshot: &ImeProfileSnapshot) -> WindowsImeProfileResult<()> {
        windows_impl::restore_profile(snapshot)
    }

    pub fn is_openless_profile_active(&self) -> WindowsImeProfileResult<bool> {
        windows_impl::is_openless_profile_active()
    }
}

#[cfg(not(target_os = "windows"))]
pub struct WindowsImeProfileManager;

#[cfg(not(target_os = "windows"))]
impl WindowsImeProfileManager {
    pub fn new() -> Self {
        Self
    }

    pub fn capture_active_profile(&self) -> WindowsImeProfileResult<ImeProfileSnapshot> {
        Err(WindowsImeProfileError::Unavailable(
            "Windows TSF profiles are only available on Windows".to_string(),
        ))
    }

    pub fn activate_openless_profile(&self) -> WindowsImeProfileResult<()> {
        Err(WindowsImeProfileError::Unavailable(
            "Windows TSF profiles are only available on Windows".to_string(),
        ))
    }

    pub fn restore_profile(&self, _snapshot: &ImeProfileSnapshot) -> WindowsImeProfileResult<()> {
        Err(WindowsImeProfileError::Unavailable(
            "Windows TSF profiles are only available on Windows".to_string(),
        ))
    }

    pub fn is_openless_profile_active(&self) -> WindowsImeProfileResult<bool> {
        Ok(false)
    }
}

/// Combine the legacy and modern restore paths' results: either succeeding counts as overall
/// success; only both failing fails, with each failure reason logged.
pub(super) fn report_restore_step_results(
    legacy_result: WindowsImeProfileResult<()>,
    modern_result: WindowsImeProfileResult<()>,
) -> WindowsImeProfileResult<()> {
    if let Err(error) = &legacy_result {
        log::warn!(
            "[windows-ime] legacy restore failed (ChangeCurrentLanguage/ActivateLanguageProfile): {error}"
        );
    }
    if let Err(error) = &modern_result {
        log::warn!("[windows-ime] modern ActivateProfile failed: {error}");
    }
    match (legacy_result, modern_result) {
        (Ok(()), _) | (_, Ok(())) => Ok(()),
        (Err(legacy_error), Err(modern_error)) => Err(WindowsImeProfileError::WindowsApi(format!(
            "both legacy and modern restore failed: legacy={legacy_error}; modern={modern_error}"
        ))),
    }
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use super::*;
    use std::ffi::c_void;
    use std::path::Path;
    use std::ptr;
    use windows::core::{GUID, HRESULT};
    use windows::Win32::Foundation::BOOL;
    use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
    use windows::Win32::UI::TextServices::{
        CLSID_TF_InputProcessorProfiles, ITfInputProcessorProfileMgr, ITfInputProcessorProfiles,
        GUID_TFCAT_TIP_KEYBOARD, TF_INPUTPROCESSORPROFILE, TF_IPPMF_DONTCARECURRENTINPUTLANGUAGE,
        TF_IPPMF_ENABLEPROFILE, TF_IPPMF_FORSESSION, TF_PROFILETYPE_INPUTPROCESSOR,
        TF_PROFILETYPE_KEYBOARDLAYOUT,
    };
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY};
    use winreg::RegKey;

    const OPENLESS_COM_INPROC_KEY: &str =
        r"Software\Classes\CLSID\{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}\InprocServer32";
    const OPENLESS_TSF_PROFILE_KEY: &str = r"Software\Microsoft\CTF\TIP\{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}\LanguageProfile\0x00000804\{9B5F5E04-23F6-47DA-9A26-D221F6C3F02E}";
    const OPENLESS_TSF_KEYBOARD_CATEGORY_KEY: &str = r"Software\Microsoft\CTF\TIP\{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}\Category\Category\{34745C63-B2F0-4784-8B67-5E12C8701A31}\{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}";
    const OPENLESS_TSF_IMMERSIVE_CATEGORY_KEY: &str = r"Software\Microsoft\CTF\TIP\{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}\Category\Category\{13A016DF-560B-46CD-947A-4C3AF1E0E35D}\{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}";
    const OPENLESS_TSF_SYSTRAY_CATEGORY_KEY: &str = r"Software\Microsoft\CTF\TIP\{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}\Category\Category\{25504FB4-7BAB-4BC1-9C69-CF81890F0EF5}\{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}";
    const OPENLESS_PROFILE_ACTIVATION_FLAGS: u32 =
        TF_IPPMF_FORSESSION | TF_IPPMF_DONTCARECURRENTINPUTLANGUAGE | TF_IPPMF_ENABLEPROFILE;
    const PROFILE_RESTORE_FLAGS: u32 = TF_IPPMF_FORSESSION | TF_IPPMF_DONTCARECURRENTINPUTLANGUAGE;

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

    pub fn capture_active_profile() -> WindowsImeProfileResult<ImeProfileSnapshot> {
        let profile = with_profile_manager(|manager| {
            let mut profile = TF_INPUTPROCESSORPROFILE::default();
            unsafe {
                manager.GetActiveProfile(&active_profile_category_guid(), &mut profile)?;
            }

            Ok(profile)
        })?;

        if profile.dwProfileType == TF_PROFILETYPE_INPUTPROCESSOR {
            Ok(ImeProfileSnapshot::text_service(
                profile.langid,
                guid_to_braced_string(profile.clsid),
                guid_to_braced_string(profile.guidProfile),
            ))
        } else {
            keyboard_layout_snapshot_from_tsf(profile.langid, profile.hkl)
        }
    }

    pub(super) fn guid_to_braced_string(guid: GUID) -> String {
        format!(
            "{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
            guid.data1,
            guid.data2,
            guid.data3,
            guid.data4[0],
            guid.data4[1],
            guid.data4[2],
            guid.data4[3],
            guid.data4[4],
            guid.data4[5],
            guid.data4[6],
            guid.data4[7],
        )
    }

    pub fn activate_openless_profile() -> WindowsImeProfileResult<()> {
        let clsid = parse_guid(OPENLESS_TEXT_SERVICE_CLSID_BRACED)?;
        let profile_guid = parse_guid(OPENLESS_PROFILE_GUID_BRACED)?;

        with_input_processor_profiles(|profiles| unsafe {
            profiles.EnableLanguageProfile(&clsid, OPENLESS_TSF_LANG_ID, &profile_guid, true)?;
            profiles.ChangeCurrentLanguage(OPENLESS_TSF_LANG_ID)?;
            profiles.ActivateLanguageProfile(&clsid, OPENLESS_TSF_LANG_ID, &profile_guid)
        })?;

        with_profile_manager(|manager| unsafe {
            manager.ActivateProfile(
                TF_PROFILETYPE_INPUTPROCESSOR,
                OPENLESS_TSF_LANG_ID,
                &clsid,
                &profile_guid,
                null_hkl(),
                OPENLESS_PROFILE_ACTIVATION_FLAGS,
            )
        })
    }

    pub fn restore_profile(snapshot: &ImeProfileSnapshot) -> WindowsImeProfileResult<()> {
        // Must stay symmetric with the activate_openless_profile path: activation calls both the
        // legacy ITfInputProcessorProfiles ChangeCurrentLanguage + ActivateLanguageProfile and
        // the modern ITfInputProcessorProfileMgr::ActivateProfile; calling only the modern one
        // doesn't update the legacy current language / active profile state, so the OS still
        // treats OpenLess as the current IME and the user's IME can't be switched back. issue #469.
        //
        // #852 hardening: legacy and modern run independently with results recorded separately;
        // a legacy failure no longer short-circuits the modern call (a legacy `?` previously
        // meant the modern ActivateProfile never ran and the whole restore failed). Either
        // succeeding counts as success: legacy success → the OS visual layer (language
        // indicator, keyboard event routing) switched back; modern success → session-level
        // activation switched back. Only both failing fails.
        let lang_id = snapshot.lang_id();

        // legacy and modern share the same resolved arguments (TextService: CLSID + profile GUID;
        // KeyboardLayout: HKL). GUID parse failure fails the whole restore, as before.
        let args = resolve_restore_args(snapshot)?;

        // legacy steps: switch the language first, then activate the specific profile for a
        // TextService (KeyboardLayout has no profile).
        let legacy_result = with_input_processor_profiles(|profiles| unsafe {
            profiles.ChangeCurrentLanguage(lang_id)?;
            if args.profile_type == TF_PROFILETYPE_INPUTPROCESSOR {
                profiles.ActivateLanguageProfile(&args.clsid, lang_id, &args.profile_guid)?;
            }
            Ok(())
        });
        let modern_result = with_profile_manager(|manager| unsafe {
            manager.ActivateProfile(
                args.profile_type,
                lang_id,
                &args.clsid,
                &args.profile_guid,
                args.hkl,
                PROFILE_RESTORE_FLAGS,
            )
        });
        report_restore_step_results(legacy_result, modern_result)
    }

    /// Resolved arguments for a single restore (shared by the legacy and modern paths).
    struct RestoreArgs {
        profile_type: u32,
        clsid: GUID,
        profile_guid: GUID,
        hkl: HKL,
    }

    /// Resolve restore arguments: TextService uses CLSID + profile GUID, KeyboardLayout uses HKL.
    fn resolve_restore_args(snapshot: &ImeProfileSnapshot) -> WindowsImeProfileResult<RestoreArgs> {
        match snapshot.kind() {
            ImeProfileKind::TextService => {
                let clsid = parse_required_guid("text service CLSID", snapshot.clsid())?;
                let profile_guid =
                    parse_required_guid("text service profile GUID", snapshot.profile_guid())?;
                Ok(RestoreArgs {
                    profile_type: TF_PROFILETYPE_INPUTPROCESSOR,
                    clsid,
                    profile_guid,
                    hkl: null_hkl(),
                })
            }
            ImeProfileKind::KeyboardLayout => {
                let hkl = HKL(snapshot.hkl().unwrap_or_default() as *mut c_void);
                Ok(RestoreArgs {
                    profile_type: TF_PROFILETYPE_KEYBOARDLAYOUT,
                    clsid: GUID::zeroed(),
                    profile_guid: GUID::zeroed(),
                    hkl,
                })
            }
        }
    }

    pub fn is_openless_profile_active() -> WindowsImeProfileResult<bool> {
        let snapshot = capture_active_profile()?;
        Ok(is_openless_profile_snapshot(&snapshot))
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
        let keyboard_category_exists = hklm
            .open_subkey_with_flags(
                OPENLESS_TSF_KEYBOARD_CATEGORY_KEY,
                KEY_READ | KEY_WOW64_64KEY,
            )
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

        if com_key.is_err() && !tip_key_exists && !keyboard_category_exists {
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

        if !keyboard_category_exists {
            return RegistrationInspection::Broken {
                dll_path: Some(dll_path),
                reason: "OpenLess TSF keyboard category registration is missing".to_string(),
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

    fn with_profile_manager<T>(
        operation: impl FnOnce(&ITfInputProcessorProfileMgr) -> windows::core::Result<T>,
    ) -> WindowsImeProfileResult<T> {
        let _com = ComApartment::initialize()?;
        let manager: ITfInputProcessorProfileMgr = unsafe {
            CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER)
        }
        .map_err(windows_api_error(
            "CoCreateInstance ITfInputProcessorProfileMgr",
        ))?;

        operation(&manager).map_err(windows_api_error("ITfInputProcessorProfileMgr operation"))
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

    fn parse_required_guid(label: &str, value: Option<&str>) -> WindowsImeProfileResult<GUID> {
        parse_guid(value.ok_or_else(|| {
            WindowsImeProfileError::WindowsApi(format!("missing {label} in saved IME profile"))
        })?)
    }

    pub(super) fn active_profile_category_guid() -> GUID {
        GUID_TFCAT_TIP_KEYBOARD
    }

    pub(super) fn keyboard_layout_snapshot_from_tsf(
        lang_id: u16,
        hkl: HKL,
    ) -> WindowsImeProfileResult<ImeProfileSnapshot> {
        let hkl_value = hkl_to_isize(hkl);
        if hkl_value == 0 {
            return Err(WindowsImeProfileError::WindowsApi(
                "active keyboard layout profile has no HKL".to_string(),
            ));
        }

        Ok(ImeProfileSnapshot::keyboard_layout(lang_id, hkl_value))
    }

    fn hkl_to_isize(hkl: HKL) -> isize {
        hkl.0 as isize
    }

    fn null_hkl() -> HKL {
        HKL(ptr::null_mut())
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

    fn text_service_snapshot() -> ImeProfileSnapshot {
        ImeProfileSnapshot::text_service(
            0x0804,
            "{11111111-1111-1111-1111-111111111111}".to_string(),
            "{22222222-2222-2222-2222-222222222222}".to_string(),
        )
    }

    #[test]
    fn text_service_constructor_sets_required_profile_data() {
        let snapshot = text_service_snapshot();

        assert_eq!(snapshot.kind(), &ImeProfileKind::TextService);
        assert_eq!(snapshot.lang_id(), 0x0804);
        assert_eq!(
            snapshot.clsid(),
            Some("{11111111-1111-1111-1111-111111111111}")
        );
        assert_eq!(
            snapshot.profile_guid(),
            Some("{22222222-2222-2222-2222-222222222222}")
        );
        assert_eq!(snapshot.hkl(), None);
    }

    #[test]
    fn keyboard_layout_constructor_sets_required_hkl_data() {
        let snapshot = ImeProfileSnapshot::keyboard_layout(0x0409, 0x0409_0409);

        assert_eq!(snapshot.kind(), &ImeProfileKind::KeyboardLayout);
        assert_eq!(snapshot.lang_id(), 0x0409);
        assert_eq!(snapshot.clsid(), None);
        assert_eq!(snapshot.profile_guid(), None);
        assert_eq!(snapshot.hkl(), Some(0x0409_0409));
    }

    #[test]
    fn restore_is_required_when_openless_was_activated() {
        assert_eq!(
            restore_decision(Some(&text_service_snapshot()), true, false),
            ProfileRestoreDecision::RestoreSavedProfile
        );
    }

    #[test]
    fn restore_is_required_after_activation_failure_with_snapshot() {
        assert_eq!(
            restore_decision(Some(&text_service_snapshot()), false, true),
            ProfileRestoreDecision::RestoreSavedProfile
        );
    }

    #[test]
    fn restore_is_skipped_when_snapshot_is_missing() {
        assert_eq!(
            restore_decision(None, true, true),
            ProfileRestoreDecision::KeepCurrentProfile
        );
    }

    #[test]
    fn restore_is_skipped_when_session_never_activated() {
        assert_eq!(
            restore_decision(Some(&text_service_snapshot()), false, false),
            ProfileRestoreDecision::KeepCurrentProfile
        );
    }

    #[test]
    fn openless_snapshot_detection_matches_exact_profile_identifiers() {
        // GUIDs differing in case/braces must also be recognized as OpenLess after normalization (sticky-state protection).
        let openless = openless_snapshot_for_test();
        assert!(is_openless_profile_snapshot(&openless));

        let other_ime = text_service_snapshot();
        assert!(!is_openless_profile_snapshot(&other_ime));

        let keyboard = ImeProfileSnapshot::keyboard_layout(0x0409, 0x0409_0409);
        assert!(!is_openless_profile_snapshot(&keyboard));
    }

    // ── Fix: the "keyboard list visibility" preference must not error when the TSF IME is not installed ──
    // "hide" (`desired == false`) + not installed previously returned Err, which propagated
    // through settings.rs's apply_keyboard_list(&prefs)? and rolled back the whole settings save
    // transaction. Regression guard.

    #[test]
    fn uninstalled_hide_request_is_noop_ok() {
        // The user wants OpenLess hidden from the keyboard list but the TSF IME isn't installed → already satisfied → Ok(()).
        assert_eq!(
            keyboard_list_pref_short_circuit(WindowsImeInstallState::NotInstalled, false),
            Some(Ok(()))
        );
    }

    #[test]
    fn uninstalled_show_request_is_noop_ok() {
        // The user wants it shown but it isn't installed → only a no-op is possible (nothing to enable) → Ok(()).
        assert_eq!(
            keyboard_list_pref_short_circuit(WindowsImeInstallState::NotInstalled, true),
            Some(Ok(()))
        );
    }

    #[test]
    fn broken_registration_short_circuits_ok_for_both_desired_values() {
        // Broken registration also means "no trusted entry in the list" → both branches short-circuit to Ok(()), never Err.
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
        // Installed → no short-circuit; hand off to the real EnableLanguageProfile change.
        assert_eq!(
            keyboard_list_pref_short_circuit(WindowsImeInstallState::Installed, false),
            None
        );
        assert_eq!(
            keyboard_list_pref_short_circuit(WindowsImeInstallState::Installed, true),
            None
        );
    }

    #[test]
    fn restore_step_results_ok_when_modern_succeeds_after_legacy_failure() {
        let result = report_restore_step_results(
            Err(WindowsImeProfileError::WindowsApi(
                "legacy failed".to_string(),
            )),
            Ok(()),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn restore_step_results_ok_when_legacy_succeeds_and_modern_fails() {
        let result = report_restore_step_results(
            Ok(()),
            Err(WindowsImeProfileError::WindowsApi(
                "modern failed".to_string(),
            )),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn restore_step_results_err_only_when_both_fail() {
        let result = report_restore_step_results(
            Err(WindowsImeProfileError::WindowsApi(
                "legacy failed".to_string(),
            )),
            Err(WindowsImeProfileError::WindowsApi(
                "modern failed".to_string(),
            )),
        );
        let err = result.unwrap_err();
        assert!(err
            .to_string()
            .contains("both legacy and modern restore failed"));
    }
}

#[cfg(all(test, target_os = "windows"))]
mod windows_tests {
    use super::*;
    use std::ffi::c_void;
    use std::ptr;
    use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
    use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
    use windows::Win32::UI::TextServices::GUID_TFCAT_TIP_KEYBOARD;

    #[test]
    fn openless_profile_identifiers_are_fixed() {
        assert_eq!(OPENLESS_TSF_LANG_ID, 0x0804);
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
    fn active_profile_capture_uses_keyboard_tip_category() {
        assert_eq!(
            windows_impl::active_profile_category_guid(),
            GUID_TFCAT_TIP_KEYBOARD
        );
    }

    #[test]
    fn keyboard_layout_snapshot_uses_tsf_profile_values() {
        let snapshot = windows_impl::keyboard_layout_snapshot_from_tsf(
            0x0411,
            HKL(0x0411_0411usize as *mut c_void),
        )
        .unwrap();

        assert_eq!(snapshot.kind(), &ImeProfileKind::KeyboardLayout);
        assert_eq!(snapshot.lang_id(), 0x0411);
        assert_eq!(snapshot.hkl(), Some(0x0411_0411));
    }

    #[test]
    fn keyboard_layout_snapshot_rejects_missing_hkl() {
        let err = windows_impl::keyboard_layout_snapshot_from_tsf(0x0409, HKL(ptr::null_mut()))
            .unwrap_err();

        assert!(err
            .to_string()
            .contains("active keyboard layout profile has no HKL"));
    }

    #[test]
    fn guid_snapshot_strings_are_canonical_and_parseable() {
        let guid = windows::core::GUID::from_u128(0x6b9f3f4f_5ee7_42d6_9c61_9f80b03a5d7d);
        let formatted = windows_impl::guid_to_braced_string(guid);

        assert_eq!(formatted, "{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}");
        assert!(parse_guid(&formatted).is_ok());
    }

    #[test]
    fn com_changed_mode_is_accepted_without_uninitializing() {
        let ownership = windows_impl::coinitialize_result_ownership(RPC_E_CHANGED_MODE).unwrap();

        assert!(!ownership.should_uninitialize);
    }
}
