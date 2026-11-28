#[tauri::command]
pub(crate) fn get_dictation_references_status() -> crate::dictation_references::Status {
    crate::dictation_references::status()
}
