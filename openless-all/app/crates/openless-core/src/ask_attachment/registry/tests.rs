use std::io::Cursor;

use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::json;

use super::*;
use crate::ask_attachment::{decode_image, AttachmentPreviewSummary};

#[test]
fn selection_preview_is_scalar_bounded_and_lease_survives_removal() {
    let registry = AttachmentRegistry::default();
    let scope = registry.open_conversation("first");
    let other = registry.open_conversation("other");
    let excerpt = "🦀".repeat(280);
    let original = format!("{excerpt}\r\nOriginal text");
    let metadata = scope
        .register_selection(SelectedText::new(original.clone(), Some("Editor".to_owned())).unwrap())
        .unwrap();
    assert_eq!(
        serde_json::to_value(&metadata).unwrap(),
        json!({
            "attachmentId": metadata.attachment_id,
            "kind": "selection",
            "displayName": "Selected text",
            "preview": { "kind": "selection", "text": excerpt, "hasMore": true },
            "sourceApp": "Editor",
            "state": "ready"
        }),
    );
    let id = &metadata.attachment_id;
    assert_eq!(
        serde_json::to_value(scope.preview(id).unwrap()).unwrap(),
        json!({ "kind": "selection", "text": original }),
    );
    assert_eq!(other.preview(id).err(), Some(AttachmentError::NotFound));
    assert_eq!(
        registry
            .lease("other", "turn", std::slice::from_ref(id))
            .err(),
        Some(AttachmentError::NotFound),
    );
    let lease = registry
        .lease("first", "turn", std::slice::from_ref(id))
        .unwrap();
    assert!(scope.remove(id));
    assert_eq!(
        registry
            .lease("first", "later", std::slice::from_ref(id))
            .err(),
        Some(AttachmentError::NotFound),
    );
    registry.end_conversation("first");
    assert_eq!(scope.list().err(), Some(AttachmentError::NotFound));
    match lease.attachments()[0].content() {
        AttachmentContent::Selection(selection) => assert_eq!(selection.text(), original),
        _ => panic!("expected selection"),
    }
    assert!(SelectedText::new("🦀".repeat(4_000), None).is_ok());
    assert_eq!(
        SelectedText::new("a".repeat(4_001), None).err(),
        Some(AttachmentError::SelectionTooLarge),
    );
}

#[tokio::test]
async fn pending_entries_cannot_be_leased_and_late_results_do_not_reappear() {
    let registry = AttachmentRegistry::default();
    let scope = registry.open_conversation("conversation");
    let ready = scope
        .register_selection(SelectedText::new("ready".into(), None).unwrap())
        .unwrap();
    let pending = scope
        .begin_file("document.pdf".into(), FileKind::Pdf)
        .unwrap();
    let id = pending.metadata().attachment_id.clone();
    let mut cancelled = pending.cancellation();
    assert_eq!(pending.metadata().state, AttachmentState::Processing);
    assert_eq!(
        registry
            .lease("conversation", "turn", &[ready.attachment_id, id.clone()])
            .err(),
        Some(AttachmentError::NotReady),
    );
    assert!(scope.remove(&id));
    assert!(cancelled.changed().await.is_err());
    assert!(pending.finish(Err(AttachmentError::PdfInvalid)).is_none());
    assert_eq!(scope.preview(&id).err(), Some(AttachmentError::NotFound));

    let pending = scope
        .begin_file("second.pdf".into(), FileKind::Pdf)
        .unwrap();
    let mut cancelled = pending.cancellation();
    registry.clear();
    assert!(cancelled.changed().await.is_err());
    assert_eq!(
        pending.processing_slot().await.err(),
        Some(AttachmentError::Cancelled)
    );
    assert!(pending.finish(Err(AttachmentError::PdfInvalid)).is_none());
    assert_eq!(
        scope.begin_file("late.pdf".into(), FileKind::Pdf).err(),
        Some(AttachmentError::NotFound),
    );
}

#[test]
fn cap_duplicates_and_error_state_are_distinct() {
    let registry = AttachmentRegistry::default();
    let scope = registry.open_conversation("conversation");
    let ready = scope
        .register_selection(SelectedText::new("ready".into(), None).unwrap())
        .unwrap();
    assert_eq!(
        registry
            .lease(
                "conversation",
                "turn",
                &[ready.attachment_id.clone(), ready.attachment_id]
            )
            .err(),
        Some(AttachmentError::DuplicateAttachment),
    );
    assert_eq!(
        registry
            .lease("conversation", "turn", &vec!["unused".to_owned(); 5])
            .err(),
        Some(AttachmentError::TooManyAttachments),
    );
    let pending = scope.begin_file("scan.pdf".into(), FileKind::Pdf).unwrap();
    let metadata = pending.finish(Err(AttachmentError::PdfNoText)).unwrap();
    assert_eq!(metadata.state, AttachmentState::Error);
    assert_eq!(metadata.error.unwrap().code, AttachmentError::PdfNoText);
    assert_eq!(
        registry
            .lease("conversation", "turn", &[metadata.attachment_id])
            .err(),
        Some(AttachmentError::PdfNoText),
    );
}

#[test]
fn images_use_real_decoder_and_preview_never_returns_source_bytes() {
    for (kind, format, extension) in [
        (FileKind::Png, image::ImageFormat::Png, "png"),
        (FileKind::Jpeg, image::ImageFormat::Jpeg, "jpg"),
    ] {
        let mut encoded = Cursor::new(Vec::new());
        image::RgbImage::from_pixel(640, 320, image::Rgb([12, 34, 56]))
            .write_to(&mut encoded, format)
            .unwrap();
        let source = encoded.into_inner();
        let image = decode_image(kind, source.clone()).unwrap();
        assert_eq!(image.dimensions(), (640, 320));
        assert_eq!(image.bytes(), source);

        let registry = AttachmentRegistry::default();
        let scope = registry.open_conversation("conversation");
        let pending = scope
            .begin_file(format!("picture.{extension}"), kind)
            .unwrap();
        let metadata = pending.finish(Ok(AttachmentContent::Image(image))).unwrap();
        assert_eq!(
            metadata.preview,
            Some(AttachmentPreviewSummary::Image {
                width: 640,
                height: 320,
                thumbnail_available: true,
            }),
        );
        let serialized = serde_json::to_value(&metadata).unwrap();
        assert!(serialized["preview"].get("dataBase64").is_none());
        let preview =
            serde_json::to_value(scope.preview(&metadata.attachment_id).unwrap()).unwrap();
        assert_eq!(preview["mimeType"], "image/png");
        let thumbnail = STANDARD
            .decode(preview["dataBase64"].as_str().unwrap())
            .unwrap();
        let thumbnail =
            image::load_from_memory_with_format(&thumbnail, image::ImageFormat::Png).unwrap();
        assert_eq!((thumbnail.width(), thumbnail.height()), (320, 160));
        assert_eq!(
            decode_image(kind, source[..16].to_vec()).err(),
            Some(AttachmentError::ImageInvalid),
        );
    }
}

#[test]
fn image_above_the_send_dimension_budget_is_resized() {
    let mut encoded = Cursor::new(Vec::new());
    image::GrayImage::new(4_000, 3_001)
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    let prepared = decode_image(FileKind::Png, encoded.into_inner()).unwrap();
    assert_eq!(prepared.dimensions(), (2048, 1537));
    assert!(prepared.bytes().len() < super::super::MAX_IMAGE_BYTES);
}
