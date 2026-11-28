#[tauri::command]
pub(crate) fn get_dictation_references_status() -> crate::dictation_references::Status {
    crate::dictation_references::status()
}

#[tauri::command]
pub(crate) fn open_dictation_references_permission_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("/usr/bin/open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
            .status().map_err(|e| e.to_string())?;
        if status.success() { Ok(()) } else { Err("unsupported".into()) }
    }
    #[cfg(not(target_os = "macos"))]
    { Err("unsupported".into()) }
}
