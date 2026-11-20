use std::io::{Read, Write};

use lopdf::{DecompressError, Document, LoadOptions, Object};
use serde::{Deserialize, Serialize};

use super::{AttachmentError, MAX_PDF_BYTES};

const MAX_PDF_PAGES: usize = 100;
const MAX_PDF_TEXT_BYTES: usize = 512 * 1024;
const MAX_PDF_DECOMPRESSED_STREAM_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_WORKER_REPLY_BYTES: usize = 6 * MAX_PDF_TEXT_BYTES + 8 * 1024;

#[derive(Deserialize, Serialize)]
pub struct PdfPage {
    pub page_number: u32,
    pub text: String,
}

#[derive(Deserialize, Serialize)]
pub struct PdfText {
    pages: Vec<PdfPage>,
}

impl PdfText {
    pub fn page_count(&self) -> u32 {
        self.pages.len() as u32
    }

    pub fn pages(&self) -> &[PdfPage] {
        &self.pages
    }

    pub fn page(&self, number: u32) -> Option<&PdfPage> {
        self.pages.get(number.checked_sub(1)? as usize)
    }
}

/// Run only in a separate process after the host installs hard resource limits.
pub fn write_worker_reply(
    resource_limits: Result<(), AttachmentError>,
    input: impl Read,
    output: impl Write,
) -> std::io::Result<()> {
    let result = resource_limits.and_then(|()| {
        let mut bytes = Vec::new();
        input
            .take((MAX_PDF_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| AttachmentError::FileUnreadable)?;
        extract(&bytes)
    });
    serde_json::to_writer(output, &result).map_err(std::io::Error::other)
}

pub fn read_worker_reply(bytes: &[u8]) -> Result<PdfText, AttachmentError> {
    if bytes.len() > MAX_WORKER_REPLY_BYTES {
        return Err(AttachmentError::PdfTooLarge);
    }
    let result: Result<PdfText, AttachmentError> =
        serde_json::from_slice(bytes).map_err(|_| AttachmentError::PdfUnavailable)?;
    let pdf = result?;
    validate_pages(&pdf.pages)?;
    Ok(pdf)
}

fn extract(bytes: &[u8]) -> Result<PdfText, AttachmentError> {
    if bytes.len() > MAX_PDF_BYTES {
        return Err(AttachmentError::PdfTooLarge);
    }
    if !bytes.starts_with(b"%PDF-") {
        return Err(AttachmentError::PdfInvalid);
    }
    let document = Document::load_mem_with_options(
        bytes,
        LoadOptions {
            strict: true,
            max_decompressed_size: Some(MAX_PDF_DECOMPRESSED_STREAM_BYTES),
            ..Default::default()
        },
    )
    .map_err(parse_error)?;
    if document.trailer.has(b"Encrypt") || document.was_encrypted() {
        return Err(AttachmentError::PdfEncrypted);
    }
    let declared_count = document
        .catalog()
        .and_then(|catalog| catalog.get_deref(b"Pages", &document))
        .and_then(Object::as_dict)
        .and_then(|pages| pages.get_deref(b"Count", &document))
        .and_then(Object::as_i64)
        .map_err(parse_error)?;
    if declared_count > MAX_PDF_PAGES as i64 {
        return Err(AttachmentError::PdfTooLarge);
    }
    let mut pages = Vec::new();
    let mut text_bytes = 0;
    for (index, page_id) in document.page_iter().take(MAX_PDF_PAGES + 1).enumerate() {
        if index >= MAX_PDF_PAGES {
            return Err(AttachmentError::PdfTooLarge);
        }
        let page = document.get_dictionary(page_id).map_err(parse_error)?;
        if page.has(b"Contents") {
            match page
                .get_deref(b"Contents", &document)
                .map_err(parse_error)?
            {
                Object::Stream(stream) if !stream.dict.has(b"F") => {}
                Object::Array(streams) => {
                    for stream in streams {
                        let id = stream.as_reference().map_err(parse_error)?;
                        let stream = document
                            .get_object(id)
                            .and_then(Object::as_stream)
                            .map_err(parse_error)?;
                        if stream.dict.has(b"F") {
                            return Err(AttachmentError::PdfInvalid);
                        }
                    }
                }
                _ => return Err(AttachmentError::PdfInvalid),
            }
        }
        let page_number = (index + 1) as u32;
        let text = document
            .extract_text_with_limit(&[page_number], MAX_PDF_DECOMPRESSED_STREAM_BYTES)
            .map_err(parse_error)?;
        let mut text = text
            .lines()
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n");
        text.truncate(text.trim_end().len());
        text_bytes += text.len();
        if text_bytes > MAX_PDF_TEXT_BYTES {
            return Err(AttachmentError::PdfTooLarge);
        }
        pages.push(PdfPage { page_number, text });
    }
    if pages.len() as i64 != declared_count {
        return Err(AttachmentError::PdfInvalid);
    }
    validate_pages(&pages)?;
    Ok(PdfText { pages })
}

fn validate_pages(pages: &[PdfPage]) -> Result<(), AttachmentError> {
    if pages.len() > MAX_PDF_PAGES {
        return Err(AttachmentError::PdfTooLarge);
    }
    let mut total = 0;
    let mut has_text = false;
    for (index, page) in pages.iter().enumerate() {
        if page.page_number != (index + 1) as u32 {
            return Err(AttachmentError::PdfInvalid);
        }
        total += page.text.len();
        if total > MAX_PDF_TEXT_BYTES {
            return Err(AttachmentError::PdfTooLarge);
        }
        has_text |= !page.text.trim().is_empty();
    }
    if has_text {
        Ok(())
    } else {
        Err(AttachmentError::PdfNoText)
    }
}

fn parse_error(error: lopdf::Error) -> AttachmentError {
    match error {
        lopdf::Error::AlreadyEncrypted
        | lopdf::Error::InvalidPassword
        | lopdf::Error::Decryption(_)
        | lopdf::Error::UnsupportedSecurityHandler(_) => AttachmentError::PdfEncrypted,
        lopdf::Error::Decompress(
            DecompressError::MemoryLimitExceeded { .. } | DecompressError::TooManyFilters { .. },
        )
        | lopdf::Error::RecursionLimit
        | lopdf::Error::ReferenceLimit => AttachmentError::PdfTooLarge,
        _ => AttachmentError::PdfInvalid,
    }
}

#[cfg(test)]
mod tests;
