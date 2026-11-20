use std::io::Cursor;

use base64::{engine::general_purpose::STANDARD, Engine};
use image::{ImageDecoder, ImageError, ImageFormat, ImageReader, Limits};

use super::{
    AttachmentError, AttachmentPreview, AttachmentPreviewSummary, FileKind, MAX_IMAGE_BYTES,
    MAX_IMAGE_SOURCE_BYTES,
};

const MAX_SOURCE_PIXELS: u64 = 40_000_000;
const COMPACT_EDGE: u32 = 2048;
const COMPACT_TARGET: usize = 3 * 1024 * 1024;
const THUMBNAIL_EDGE: u32 = 320;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageMime {
    Png,
    Jpeg,
}

impl ImageMime {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
        }
    }
}

pub struct VerifiedImage {
    mime: ImageMime,
    bytes: Vec<u8>,
    width: u32,
    height: u32,
    thumbnail_base64: String,
}

impl VerifiedImage {
    pub fn mime(&self) -> ImageMime {
        self.mime
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub(super) fn summary(&self) -> AttachmentPreviewSummary {
        AttachmentPreviewSummary::Image {
            width: self.width,
            height: self.height,
            thumbnail_available: true,
        }
    }

    pub(super) fn preview(&self) -> AttachmentPreview {
        AttachmentPreview::Image {
            mime_type: "image/png",
            data_base64: self.thumbnail_base64.clone(),
        }
    }
}

pub fn decode_image(kind: FileKind, bytes: Vec<u8>) -> Result<VerifiedImage, AttachmentError> {
    let (format, mut mime) = match kind {
        FileKind::Png => (ImageFormat::Png, ImageMime::Png),
        FileKind::Jpeg => (ImageFormat::Jpeg, ImageMime::Jpeg),
        FileKind::Pdf => return Err(AttachmentError::UnsupportedFile),
    };
    if bytes.len() > MAX_IMAGE_SOURCE_BYTES {
        return Err(AttachmentError::ImageTooLarge);
    }
    if image::guess_format(&bytes).ok() != Some(format) {
        return Err(AttachmentError::ImageInvalid);
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(24_000);
    limits.max_image_height = Some(24_000);
    limits.max_alloc = Some(256 * 1024 * 1024);
    let mut reader = ImageReader::with_format(Cursor::new(&bytes), format);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(decode_error)?;
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_SOURCE_PIXELS {
        return Err(AttachmentError::ImageTooLarge);
    }
    let orientation = decoder.orientation().map_err(decode_error)?;
    let mut image = image::DynamicImage::from_decoder(decoder).map_err(decode_error)?;
    image.apply_orientation(orientation);
    let mut bytes = bytes;
    if bytes.len() > COMPACT_TARGET
        || u64::from(width) * u64::from(height) > 12_000_000
        || width.max(height) > 8_000
    {
        image = image.resize(
            COMPACT_EDGE,
            COMPACT_EDGE,
            image::imageops::FilterType::Triangle,
        );
        // Preserve transparency and the legibility of PNG screenshots where possible.
        let mut png = Cursor::new(Vec::new());
        if mime == ImageMime::Png {
            image
                .write_to(&mut png, ImageFormat::Png)
                .map_err(decode_error)?;
        }
        if mime == ImageMime::Png && png.get_ref().len() <= COMPACT_TARGET {
            bytes = png.into_inner();
        } else {
            if image.color().has_alpha() {
                let mut opaque = image::RgbImage::new(image.width(), image.height());
                for (src, dst) in image.to_rgba8().pixels().zip(opaque.pixels_mut()) {
                    let alpha = u16::from(src[3]);
                    *dst = image::Rgb(std::array::from_fn(|channel| {
                        ((u16::from(src[channel]) * alpha + 255 * (255 - alpha)) / 255) as u8
                    }));
                }
                image = image::DynamicImage::ImageRgb8(opaque);
            }
            let rgb = image.to_rgb8();
            bytes = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 86)
                .encode_image(&rgb)
                .map_err(decode_error)?;
            mime = ImageMime::Jpeg;
        }
        if bytes.len() > MAX_IMAGE_BYTES {
            return Err(AttachmentError::ImageTooLarge);
        }
    }
    let (width, height) = (image.width(), image.height());
    let thumbnail = if width > THUMBNAIL_EDGE || height > THUMBNAIL_EDGE {
        image.thumbnail(THUMBNAIL_EDGE, THUMBNAIL_EDGE).into_rgba8()
    } else {
        image.into_rgba8()
    };
    let mut encoded = Cursor::new(Vec::new());
    thumbnail
        .write_to(&mut encoded, ImageFormat::Png)
        .map_err(decode_error)?;
    Ok(VerifiedImage {
        mime,
        bytes,
        width,
        height,
        thumbnail_base64: STANDARD.encode(encoded.into_inner()),
    })
}

fn decode_error(error: ImageError) -> AttachmentError {
    match error {
        ImageError::Limits(_) => AttachmentError::ImageTooLarge,
        _ => AttachmentError::ImageInvalid,
    }
}
