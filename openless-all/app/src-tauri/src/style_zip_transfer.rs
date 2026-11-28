//! Android SAF destinations are documents, not filesystem paths.
//! Keep encoding/validation in Core; the host only chooses destination IO.

pub(crate) fn export_zip<E>(
    target: &str,
    encode: impl FnOnce() -> Result<Vec<u8>, E>,
    write_document: impl FnOnce(&str, &[u8]) -> Result<(), E>,
    write_file: impl FnOnce() -> Result<(), E>,
) -> Result<(), E> {
    if target.starts_with("content://") {
        let bytes = encode()?;
        write_document(target, &bytes)
    } else {
        write_file()
    }
}
