use lopdf::{
    content::{Content, Operation},
    dictionary, Dictionary, EncryptionState, EncryptionVersion, Permissions, Stream,
};

use super::*;

fn document_with_pages(pages: &[Option<&str>]) -> Document {
    let mut document = Document::with_version("1.5");
    let pages_id = document.new_object_id();
    let font = document.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let resources = document.add_object(dictionary! { "Font" => dictionary! { "F1" => font } });
    let kids: Vec<Object> = pages
        .iter()
        .map(|text| {
            let mut page = dictionary! { "Type" => "Page", "Parent" => pages_id };
            if let Some(text) = text {
                let content = Content {
                    operations: vec![
                        Operation::new("BT", vec![]),
                        Operation::new("Tf", vec!["F1".into(), 12.into()]),
                        Operation::new("Tj", vec![Object::string_literal(*text)]),
                        Operation::new("ET", vec![]),
                    ],
                };
                let content =
                    document.add_object(Stream::new(Dictionary::new(), content.encode().unwrap()));
                page.set("Contents", content);
            }
            document.add_object(page).into()
        })
        .collect();
    document.objects.insert(
        pages_id,
        dictionary! {
            "Type" => "Pages",
            "Kids" => kids,
            "Count" => pages.len() as i64,
            "Resources" => resources,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        }
        .into(),
    );
    let catalog = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    document.trailer.set("Root", catalog);
    document
}

fn save(mut document: Document) -> Vec<u8> {
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).unwrap();
    bytes
}

#[test]
fn worker_extracts_real_page_text_without_renumbering_blank_pages() {
    let pdf = save(document_with_pages(&[
        Some("Hello"),
        None,
        Some("Third page"),
    ]));
    let mut reply = Vec::new();
    write_worker_reply(Ok(()), pdf.as_slice(), &mut reply).unwrap();
    let text = read_worker_reply(&reply).unwrap();
    assert_eq!(text.page_count(), 3);
    assert!(text.page(0).is_none());
    assert_eq!(text.page(1).unwrap().text.trim(), "Hello");
    assert_eq!(text.page(2).unwrap().text, "");
    assert_eq!(text.page(3).unwrap().text.trim(), "Third page");
    assert!(text.page(4).is_none());
}

#[test]
fn distinguishes_no_text_damaged_encrypted_and_page_limit() {
    assert_eq!(
        extract(&save(document_with_pages(&[None]))).err(),
        Some(AttachmentError::PdfNoText),
    );
    assert_eq!(
        extract(b"%PDF-1.5\nbroken").err(),
        Some(AttachmentError::PdfInvalid),
    );
    assert_eq!(
        extract(&save(document_with_pages(&[None; MAX_PDF_PAGES + 1]))).err(),
        Some(AttachmentError::PdfTooLarge),
    );
    for user_password in ["password", ""] {
        let mut document = document_with_pages(&[Some("Secret")]);
        document.trailer.set(
            "ID",
            vec![
                Object::string_literal("first"),
                Object::string_literal("second"),
            ],
        );
        let state = EncryptionState::try_from(EncryptionVersion::V1 {
            document: &document,
            owner_password: "owner",
            user_password,
            permissions: Permissions::all(),
        })
        .unwrap();
        document.encrypt(&state).unwrap();
        assert_eq!(
            extract(&save(document)).err(),
            Some(AttachmentError::PdfEncrypted)
        );
    }
}

#[test]
fn bounds_pdf_extracted_text_stream_and_ipc_reply() {
    let oversized = "text".repeat(MAX_PDF_TEXT_BYTES / 4 + 1);
    assert_eq!(
        extract(&save(document_with_pages(&[Some(&oversized)]))).err(),
        Some(AttachmentError::PdfTooLarge),
    );
    let mut document = document_with_pages(&[Some("Healthy page")]);
    let page_id = *document.get_pages().get(&1).unwrap();
    let stream_id = document.get_page_contents(page_id)[0];
    let mut oversized_stream = Stream::new(
        Dictionary::new(),
        vec![b' '; MAX_PDF_DECOMPRESSED_STREAM_BYTES + 1],
    );
    oversized_stream.compress().unwrap();
    document.objects.insert(stream_id, oversized_stream.into());
    assert_eq!(
        extract(&save(document)).err(),
        Some(AttachmentError::PdfTooLarge)
    );
    assert_eq!(
        read_worker_reply(&vec![0; MAX_WORKER_REPLY_BYTES + 1]).err(),
        Some(AttachmentError::PdfTooLarge),
    );
}
