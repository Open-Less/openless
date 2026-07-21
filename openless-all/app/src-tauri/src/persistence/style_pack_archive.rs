//! Resource-bounded parser for the untrusted style-pack ZIP format.

use std::collections::HashSet;
use std::fs;
use std::io::{Cursor, Read, Seek};
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};

use super::{atomic_write, ensure_dir};
use crate::types::{PolishMode, StylePackExample};

pub(crate) const STYLE_PACK_ARCHIVE_MAX_COMPRESSED_BYTES: usize = 512 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 16;
pub(super) const MAX_ENTRY_UNCOMPRESSED_BYTES: usize = 128 * 1024;
pub(super) const MAX_MANIFEST_BYTES: usize = 32 * 1024;
pub(super) const MAX_PROMPT_BYTES: usize = 64 * 1024;
pub(super) const MAX_EXAMPLES_BYTES: usize = 64 * 1024;
pub(super) const MAX_ICON_BYTES: usize = 64 * 1024;
const MAX_ENTRY_NAME_BYTES: usize = 255;
const MAX_ICON_DIMENSION: u32 = 1024;
const MAX_ICON_PIXELS: u64 = 1024 * 1024;
pub(super) const MAX_TOTAL_UNCOMPRESSED_BYTES: usize = 256 * 1024;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct StylePackArchiveManifest {
    pub(super) schema_version: u32,
    pub(super) id: String,
    pub(super) name: String,
    pub(super) description: String,
    pub(super) author: Option<String>,
    pub(super) version: String,
    pub(super) base_mode: PolishMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) selection_prompt: Option<String>,
    pub(super) tags: Vec<String>,
    pub(super) prompt_file: String,
    pub(super) examples_file: String,
    pub(super) icon_file: Option<String>,
    pub(super) recommended_model: Option<String>,
    pub(super) compatible_app_version: Option<String>,
    /// Marketplace 上游关系。旧 ZIP 没有此字段时自动为 None；
    /// 兼容早期口误/拼写包里可能出现的 `orion*` 字段名。
    #[serde(
        default,
        alias = "orionPackId",
        alias = "orion_pack_id",
        alias = "origin_pack_id"
    )]
    pub(super) origin_pack_id: Option<String>,
    #[serde(
        default,
        alias = "orionAuthorLogin",
        alias = "orion_author_login",
        alias = "origin_author_login"
    )]
    pub(super) origin_author_login: Option<String>,
}

pub(super) struct ParsedStylePackArchive {
    pub(super) manifest: StylePackArchiveManifest,
    pub(super) prompt: String,
    pub(super) examples: Vec<StylePackExample>,
    pub(super) icon: Option<StylePackIcon>,
}

pub(super) struct StylePackIcon {
    extension: String,
    bytes: Vec<u8>,
}

#[derive(Default)]
pub(super) struct StreamBudget {
    pub(super) total: usize,
}

pub(super) fn read_style_pack_archive(zip_path: &Path) -> Result<ParsedStylePackArchive> {
    let compressed = read_compressed_archive_bounded(zip_path)?;
    let mut archive = zip::ZipArchive::new(Cursor::new(compressed.as_slice()))
        .context("open style pack zip archive")?;
    preflight_physical_central_directory(&compressed, &archive)?;
    let entry_names = preflight_archive_metadata(&mut archive)?;
    let mut stream_budget = StreamBudget::default();

    let manifest_bytes = read_archive_entry(
        &mut archive,
        "manifest.json",
        MAX_MANIFEST_BYTES,
        &mut stream_budget,
    )?;
    let manifest: StylePackArchiveManifest = serde_json::from_slice(&manifest_bytes)
        .context("decode style pack zip entry failed: manifest.json")?;
    if manifest.schema_version != 1 {
        bail!(
            "unsupported style pack manifest schema version: {}",
            manifest.schema_version
        );
    }

    validate_manifest_reference(&manifest.prompt_file, &entry_names, ReferenceKind::Prompt)?;
    validate_manifest_reference(
        &manifest.examples_file,
        &entry_names,
        ReferenceKind::Examples,
    )?;
    if manifest.prompt_file == manifest.examples_file {
        bail!("style pack prompt and examples entries must be distinct");
    }

    let prompt_bytes = read_archive_entry(
        &mut archive,
        &manifest.prompt_file,
        MAX_PROMPT_BYTES,
        &mut stream_budget,
    )?;
    let prompt = String::from_utf8(prompt_bytes).with_context(|| {
        format!(
            "style pack prompt is not valid UTF-8: {}",
            manifest.prompt_file
        )
    })?;

    let examples_bytes = read_archive_entry(
        &mut archive,
        &manifest.examples_file,
        MAX_EXAMPLES_BYTES,
        &mut stream_budget,
    )?;
    let examples =
        serde_json::from_slice::<Vec<StylePackExample>>(&examples_bytes).with_context(|| {
            format!(
                "decode style pack zip entry failed: {}",
                manifest.examples_file
            )
        })?;

    let icon = if let Some(icon_file) = manifest.icon_file.as_deref() {
        let extension = validate_manifest_reference(icon_file, &entry_names, ReferenceKind::Icon)?;
        let bytes =
            read_archive_entry(&mut archive, icon_file, MAX_ICON_BYTES, &mut stream_budget)?;
        validate_icon_content(&extension, &bytes)?;
        Some(StylePackIcon { extension, bytes })
    } else {
        None
    };

    Ok(ParsedStylePackArchive {
        manifest,
        prompt,
        examples,
        icon,
    })
}

const CENTRAL_DIRECTORY_HEADER_SIGNATURE: &[u8; 4] = b"PK\x01\x02";
const ZIP64_END_SIGNATURE: &[u8; 4] = b"PK\x06\x06";
const ZIP64_LOCATOR_SIGNATURE: &[u8; 4] = b"PK\x06\x07";
const END_OF_CENTRAL_DIRECTORY_SIGNATURE: &[u8; 4] = b"PK\x05\x06";
const CENTRAL_DIRECTORY_HEADER_LEN: usize = 46;
const END_OF_CENTRAL_DIRECTORY_LEN: usize = 22;
const ZIP64_END_MIN_LEN: usize = 56;
const ZIP64_LOCATOR_LEN: usize = 20;

fn preflight_physical_central_directory(
    bytes: &[u8],
    archive: &zip::ZipArchive<Cursor<&[u8]>>,
) -> Result<()> {
    let comment_len = archive.comment().len();
    let eocd_len = END_OF_CENTRAL_DIRECTORY_LEN
        .checked_add(comment_len)
        .ok_or_else(|| anyhow!("style pack zip EOCD length overflow"))?;
    let eocd_offset = bytes
        .len()
        .checked_sub(eocd_len)
        .ok_or_else(|| anyhow!("style pack zip EOCD is truncated"))?;
    if bytes.get(eocd_offset..eocd_offset + 4)
        != Some(END_OF_CENTRAL_DIRECTORY_SIGNATURE.as_slice())
    {
        bail!("style pack zip EOCD is not anchored at the end of the archive");
    }
    let declared_comment_len = read_u16(bytes, eocd_offset + 20)? as usize;
    if declared_comment_len != comment_len {
        bail!("style pack zip EOCD comment length is inconsistent");
    }

    let disk_number = read_u16(bytes, eocd_offset + 4)?;
    let central_disk = read_u16(bytes, eocd_offset + 6)?;
    if disk_number != 0 || central_disk != 0 {
        bail!("multi-disk style pack zip archives are not supported");
    }
    let entries_on_disk = read_u16(bytes, eocd_offset + 8)?;
    let total_entries = read_u16(bytes, eocd_offset + 10)?;
    if entries_on_disk != total_entries {
        bail!("multi-disk style pack zip archives are not supported");
    }

    let central_start = usize::try_from(archive.central_directory_start())
        .map_err(|_| anyhow!("style pack zip central directory offset is unsupported"))?;
    let archive_offset = usize::try_from(archive.offset())
        .map_err(|_| anyhow!("style pack zip archive offset is unsupported"))?;
    if central_start > eocd_offset {
        bail!("style pack zip central directory starts beyond its EOCD");
    }

    let is_zip64 = entries_on_disk == u16::MAX
        || total_entries == u16::MAX
        || read_u32(bytes, eocd_offset + 12)? == u32::MAX
        || read_u32(bytes, eocd_offset + 16)? == u32::MAX;
    let (central_end, declared_entries, declared_size, declared_relative_offset) = if is_zip64 {
        parse_zip64_footer(bytes, eocd_offset, archive_offset)?
    } else {
        (
            eocd_offset,
            total_entries as u64,
            read_u32(bytes, eocd_offset + 12)? as u64,
            read_u32(bytes, eocd_offset + 16)? as u64,
        )
    };
    let expected_central_start = (archive_offset as u64)
        .checked_add(declared_relative_offset)
        .ok_or_else(|| anyhow!("style pack zip central directory offset overflow"))?;
    if expected_central_start != central_start as u64 {
        bail!("style pack zip central directory offset is inconsistent");
    }
    let actual_central_size = central_end
        .checked_sub(central_start)
        .ok_or_else(|| anyhow!("style pack zip central directory bounds are inconsistent"))?;
    if declared_size != actual_central_size as u64 {
        bail!("style pack zip central directory size is inconsistent");
    }

    let mut cursor = central_start;
    let mut physical_count = 0usize;
    while cursor < central_end {
        if bytes.get(cursor..cursor + 4) != Some(CENTRAL_DIRECTORY_HEADER_SIGNATURE.as_slice()) {
            bail!("style pack zip central directory contains a malformed physical entry");
        }
        physical_count = physical_count
            .checked_add(1)
            .ok_or_else(|| anyhow!("style pack zip physical entry count overflow"))?;
        if physical_count > MAX_ARCHIVE_ENTRIES {
            bail!("style pack archive contains more than {MAX_ARCHIVE_ENTRIES} physical entries");
        }

        let name_len = read_u16(bytes, cursor + 28)? as usize;
        let extra_len = read_u16(bytes, cursor + 30)? as usize;
        let entry_comment_len = read_u16(bytes, cursor + 32)? as usize;
        let disk_start = read_u16(bytes, cursor + 34)?;
        if disk_start != 0 && disk_start != u16::MAX {
            bail!("multi-disk style pack zip entries are not supported");
        }
        let name_start = cursor
            .checked_add(CENTRAL_DIRECTORY_HEADER_LEN)
            .ok_or_else(|| anyhow!("style pack zip central entry offset overflow"))?;
        let record_len = CENTRAL_DIRECTORY_HEADER_LEN
            .checked_add(name_len)
            .and_then(|value| value.checked_add(extra_len))
            .and_then(|value| value.checked_add(entry_comment_len))
            .ok_or_else(|| anyhow!("style pack zip central entry length overflow"))?;
        let record_end = cursor
            .checked_add(record_len)
            .filter(|end| *end <= central_end)
            .ok_or_else(|| anyhow!("style pack zip central entry is truncated"))?;
        let name_end = name_start
            .checked_add(name_len)
            .filter(|end| *end <= record_end)
            .ok_or_else(|| anyhow!("style pack zip central entry name is truncated"))?;
        bytes
            .get(name_start..name_end)
            .ok_or_else(|| anyhow!("style pack zip central entry name is truncated"))?;
        cursor = record_end;
    }

    if cursor != central_end || physical_count as u64 != declared_entries {
        bail!("style pack zip physical entry count is inconsistent");
    }
    if physical_count != archive.len() {
        bail!("duplicate physical style pack zip entries were collapsed while decoding");
    }
    Ok(())
}

fn parse_zip64_footer(
    bytes: &[u8],
    eocd_offset: usize,
    archive_offset: usize,
) -> Result<(usize, u64, u64, u64)> {
    let locator_offset = eocd_offset
        .checked_sub(ZIP64_LOCATOR_LEN)
        .ok_or_else(|| anyhow!("style pack ZIP64 locator is truncated"))?;
    if bytes.get(locator_offset..locator_offset + 4) != Some(ZIP64_LOCATOR_SIGNATURE.as_slice()) {
        bail!("style pack ZIP64 locator is missing");
    }
    if read_u32(bytes, locator_offset + 4)? != 0 || read_u32(bytes, locator_offset + 16)? != 1 {
        bail!("multi-disk style pack ZIP64 archives are not supported");
    }
    let zip64_relative_offset = usize::try_from(read_u64(bytes, locator_offset + 8)?)
        .map_err(|_| anyhow!("style pack ZIP64 EOCD offset is unsupported"))?;
    let zip64_offset = archive_offset
        .checked_add(zip64_relative_offset)
        .ok_or_else(|| anyhow!("style pack ZIP64 EOCD offset overflow"))?;
    let zip64_signature_end = zip64_offset
        .checked_add(4)
        .ok_or_else(|| anyhow!("style pack ZIP64 EOCD offset overflow"))?;
    if bytes.get(zip64_offset..zip64_signature_end) != Some(ZIP64_END_SIGNATURE.as_slice()) {
        bail!("style pack ZIP64 EOCD is missing");
    }
    let zip64_payload_len = usize::try_from(read_u64(bytes, zip64_offset + 4)?)
        .map_err(|_| anyhow!("style pack ZIP64 EOCD length is unsupported"))?;
    let zip64_total_len = 12usize
        .checked_add(zip64_payload_len)
        .ok_or_else(|| anyhow!("style pack ZIP64 EOCD length overflow"))?;
    if zip64_total_len < ZIP64_END_MIN_LEN
        || zip64_offset.checked_add(zip64_total_len) != Some(locator_offset)
    {
        bail!("style pack ZIP64 EOCD bounds are inconsistent");
    }
    if read_u32(bytes, zip64_offset + 16)? != 0 || read_u32(bytes, zip64_offset + 20)? != 0 {
        bail!("multi-disk style pack ZIP64 archives are not supported");
    }
    let entries_on_disk = read_u64(bytes, zip64_offset + 24)?;
    let total_entries = read_u64(bytes, zip64_offset + 32)?;
    if entries_on_disk != total_entries {
        bail!("multi-disk style pack ZIP64 archives are not supported");
    }
    Ok((
        zip64_offset,
        total_entries,
        read_u64(bytes, zip64_offset + 40)?,
        read_u64(bytes, zip64_offset + 48)?,
    ))
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16> {
    let end = offset
        .checked_add(2)
        .ok_or_else(|| anyhow!("style pack zip integer offset overflow"))?;
    let value = bytes
        .get(offset..end)
        .ok_or_else(|| anyhow!("style pack zip metadata is truncated"))?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| anyhow!("style pack zip integer offset overflow"))?;
    let value = bytes
        .get(offset..end)
        .ok_or_else(|| anyhow!("style pack zip metadata is truncated"))?;
    let value: [u8; 4] = value
        .try_into()
        .map_err(|_| anyhow!("style pack zip 32-bit field has an invalid length"))?;
    Ok(u32::from_le_bytes(value))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64> {
    let end = offset
        .checked_add(8)
        .ok_or_else(|| anyhow!("style pack zip integer offset overflow"))?;
    let value = bytes
        .get(offset..end)
        .ok_or_else(|| anyhow!("style pack zip metadata is truncated"))?;
    let value: [u8; 8] = value
        .try_into()
        .map_err(|_| anyhow!("style pack zip 64-bit field has an invalid length"))?;
    Ok(u64::from_le_bytes(value))
}

fn read_compressed_archive_bounded(zip_path: &Path) -> Result<Vec<u8>> {
    let file = fs::File::open(zip_path)
        .with_context(|| format!("open style pack zip failed: {}", zip_path.display()))?;
    let declared_size = file
        .metadata()
        .with_context(|| format!("read style pack zip metadata: {}", zip_path.display()))?
        .len();
    if declared_size > STYLE_PACK_ARCHIVE_MAX_COMPRESSED_BYTES as u64 {
        bail!(
            "style pack archive compressed size {declared_size} exceeds {} bytes",
            STYLE_PACK_ARCHIVE_MAX_COMPRESSED_BYTES
        );
    }

    let mut bytes = Vec::with_capacity(declared_size as usize);
    file.take(STYLE_PACK_ARCHIVE_MAX_COMPRESSED_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("read style pack zip failed: {}", zip_path.display()))?;
    if bytes.len() > STYLE_PACK_ARCHIVE_MAX_COMPRESSED_BYTES {
        bail!(
            "style pack archive streamed compressed size {} exceeds {} bytes",
            bytes.len(),
            STYLE_PACK_ARCHIVE_MAX_COMPRESSED_BYTES
        );
    }
    Ok(bytes)
}

fn preflight_archive_metadata<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Result<HashSet<String>> {
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        bail!(
            "style pack archive contains {} entries; maximum is {MAX_ARCHIVE_ENTRIES}",
            archive.len()
        );
    }

    let mut entry_names = HashSet::with_capacity(archive.len());
    let mut total_declared = 0usize;
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .with_context(|| format!("read style pack zip entry metadata at index {index}"))?;
        let name = file.name().to_string();
        validate_entry_name(&name, file.is_dir())?;
        if file.enclosed_name().is_none() {
            bail!("unsafe style pack zip entry name: {name}");
        }
        if file.is_symlink() {
            bail!("unsafe style pack zip symlink entry: {name}");
        }
        if file.encrypted() {
            bail!("encrypted style pack zip entries are not supported: {name}");
        }
        if !entry_names.insert(name.clone()) {
            bail!("duplicate style pack zip entry name: {name}");
        }

        let declared = usize::try_from(file.size())
            .map_err(|_| anyhow!("style pack zip entry {name} declared size is unsupported"))?;
        if file.is_dir() && declared != 0 {
            bail!("style pack zip directory entry {name} has non-zero size");
        }
        if declared > MAX_ENTRY_UNCOMPRESSED_BYTES {
            bail!(
                "style pack zip entry {name} declared size {declared} exceeds {MAX_ENTRY_UNCOMPRESSED_BYTES} bytes"
            );
        }
        total_declared = total_declared
            .checked_add(declared)
            .ok_or_else(|| anyhow!("style pack archive declared uncompressed size overflow"))?;
        if total_declared > MAX_TOTAL_UNCOMPRESSED_BYTES {
            bail!(
                "style pack archive total declared uncompressed size {total_declared} exceeds {MAX_TOTAL_UNCOMPRESSED_BYTES} bytes"
            );
        }
    }

    if !entry_names.contains("manifest.json") {
        bail!("missing style pack zip entry: manifest.json");
    }
    Ok(entry_names)
}

fn validate_entry_name(name: &str, is_dir: bool) -> Result<()> {
    if name.is_empty()
        || name.len() > MAX_ENTRY_NAME_BYTES
        || name.starts_with('/')
        || name.starts_with('\\')
        || name.contains('\\')
        || name.contains('\0')
        || name.chars().any(char::is_control)
    {
        bail!("unsafe style pack zip entry name: {name:?}");
    }

    let normalized = if is_dir {
        name.strip_suffix('/').unwrap_or(name)
    } else {
        if name.ends_with('/') {
            bail!("unsafe style pack zip entry name: {name:?}");
        }
        name
    };
    if normalized.is_empty()
        || normalized.split('/').any(|segment| {
            segment.is_empty() || segment == "." || segment == ".." || segment.contains(':')
        })
    {
        bail!("unsafe style pack zip entry name: {name:?}");
    }
    Ok(())
}

enum ReferenceKind {
    Prompt,
    Examples,
    Icon,
}

fn validate_manifest_reference(
    name: &str,
    entry_names: &HashSet<String>,
    kind: ReferenceKind,
) -> Result<String> {
    validate_entry_name(name, false)?;
    if name == "manifest.json" {
        bail!("style pack manifest cannot select itself as a content entry");
    }
    if !entry_names.contains(name) {
        bail!("missing style pack zip entry: {name}");
    }

    let extension = Path::new(name)
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| anyhow!("style pack zip entry has no extension: {name}"))?;
    match kind {
        ReferenceKind::Prompt if !matches!(extension.as_str(), "md" | "txt") => {
            bail!("style pack prompt entry must be .md or .txt: {name}")
        }
        ReferenceKind::Examples if extension != "json" => {
            bail!("style pack examples entry must be .json: {name}")
        }
        ReferenceKind::Icon => {
            let segments = name.split('/').collect::<Vec<_>>();
            if segments.len() != 2 || segments[0] != "assets" {
                bail!("style pack icon must be directly inside assets/: {name}");
            }
            if !matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "webp") {
                bail!(
                    "unsupported style pack icon extension .{extension}; allowed: png, jpg, jpeg, webp"
                );
            }
        }
        _ => {}
    }
    Ok(extension)
}

fn read_archive_entry<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    entry_name: &str,
    entry_limit: usize,
    stream_budget: &mut StreamBudget,
) -> Result<Vec<u8>> {
    let mut file = archive
        .by_name(entry_name)
        .with_context(|| format!("missing style pack zip entry: {entry_name}"))?;
    let declared_size = usize::try_from(file.size())
        .map_err(|_| anyhow!("style pack zip entry {entry_name} declared size is unsupported"))?;
    read_stream_bounded(
        &mut file,
        entry_name,
        declared_size,
        entry_limit,
        stream_budget,
    )
}

pub(super) fn read_stream_bounded<R: Read>(
    reader: &mut R,
    entry_name: &str,
    declared_size: usize,
    entry_limit: usize,
    stream_budget: &mut StreamBudget,
) -> Result<Vec<u8>> {
    if declared_size > entry_limit {
        bail!(
            "style pack zip entry {entry_name} declared size {declared_size} exceeds {entry_limit} bytes"
        );
    }

    let remaining_total = MAX_TOTAL_UNCOMPRESSED_BYTES.saturating_sub(stream_budget.total);
    let read_ceiling = entry_limit.min(remaining_total);
    let mut bytes = Vec::with_capacity(declared_size.min(read_ceiling));
    reader
        .take(read_ceiling as u64 + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("read style pack zip entry failed: {entry_name}"))?;

    if bytes.len() > entry_limit {
        bail!(
            "style pack zip entry {entry_name} streamed size {} exceeds {entry_limit} bytes",
            bytes.len()
        );
    }
    if bytes.len() > remaining_total {
        bail!(
            "style pack archive total streamed uncompressed size exceeds {MAX_TOTAL_UNCOMPRESSED_BYTES} bytes while reading {entry_name}"
        );
    }
    if bytes.len() != declared_size {
        bail!(
            "style pack zip entry {entry_name} streamed size {} does not match declared size {declared_size}",
            bytes.len()
        );
    }
    stream_budget.total += bytes.len();
    Ok(bytes)
}

fn validate_icon_content(extension: &str, bytes: &[u8]) -> Result<()> {
    let (width, height) = match extension {
        "png" => validate_png(bytes)?,
        "jpg" | "jpeg" => validate_jpeg(bytes)?,
        "webp" => validate_webp(bytes)?,
        _ => bail!("unsupported style pack icon extension: {extension}"),
    };
    if width == 0
        || height == 0
        || width > MAX_ICON_DIMENSION
        || height > MAX_ICON_DIMENSION
        || u64::from(width) * u64::from(height) > MAX_ICON_PIXELS
    {
        bail!(
            "style pack icon dimensions {width}x{height} exceed {MAX_ICON_DIMENSION}x{MAX_ICON_DIMENSION}"
        );
    }
    Ok(())
}

fn validate_png(bytes: &[u8]) -> Result<(u32, u32)> {
    const SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if !bytes.starts_with(SIGNATURE) {
        bail!("style pack icon content is not a PNG image");
    }

    let mut offset = SIGNATURE.len();
    let mut dimensions = None;
    let mut saw_iend = false;
    while offset.checked_add(12).is_some_and(|end| end <= bytes.len()) {
        let length = u32::from_be_bytes(
            bytes[offset..offset + 4]
                .try_into()
                .expect("four-byte PNG length"),
        ) as usize;
        let end = offset
            .checked_add(12)
            .and_then(|base| base.checked_add(length))
            .ok_or_else(|| anyhow!("style pack PNG chunk length overflow"))?;
        if end > bytes.len() {
            bail!("style pack icon contains a truncated PNG chunk");
        }
        let chunk_type = &bytes[offset + 4..offset + 8];
        let chunk_data = &bytes[offset + 8..offset + 8 + length];
        if offset == SIGNATURE.len() && (chunk_type != b"IHDR" || length != 13) {
            bail!("style pack icon PNG is missing a valid IHDR chunk");
        }
        if chunk_type == b"IHDR" {
            if length != 13 || dimensions.is_some() {
                bail!("style pack icon PNG contains a duplicate or invalid IHDR chunk");
            }
            dimensions = Some((
                u32::from_be_bytes(chunk_data[0..4].try_into().expect("PNG width")),
                u32::from_be_bytes(chunk_data[4..8].try_into().expect("PNG height")),
            ));
        } else if chunk_type == b"acTL" {
            bail!("animated PNG style pack icons are not supported");
        } else if chunk_type == b"IEND" {
            if length != 0 || end != bytes.len() {
                bail!("style pack icon PNG has an invalid IEND chunk");
            }
            saw_iend = true;
            break;
        }
        offset = end;
    }
    if !saw_iend {
        bail!("style pack icon PNG is missing IEND");
    }
    dimensions.ok_or_else(|| anyhow!("style pack icon PNG is missing dimensions"))
}

fn validate_jpeg(bytes: &[u8]) -> Result<(u32, u32)> {
    if bytes.len() < 4 || !bytes.starts_with(&[0xff, 0xd8]) || !bytes.ends_with(&[0xff, 0xd9]) {
        bail!("style pack icon content is not a complete JPEG image");
    }
    let mut offset = 2usize;
    while offset + 1 < bytes.len() {
        if bytes[offset] != 0xff {
            offset += 1;
            continue;
        }
        while offset < bytes.len() && bytes[offset] == 0xff {
            offset += 1;
        }
        if offset >= bytes.len() {
            break;
        }
        let marker = bytes[offset];
        offset += 1;
        if marker == 0xd9 {
            break;
        }
        if marker == 0x00 || marker == 0x01 || (0xd0..=0xd8).contains(&marker) {
            continue;
        }
        if offset + 2 > bytes.len() {
            bail!("style pack icon contains a truncated JPEG segment");
        }
        let segment_length = u16::from_be_bytes([bytes[offset], bytes[offset + 1]]) as usize;
        if segment_length < 2 || offset + segment_length > bytes.len() {
            bail!("style pack icon contains an invalid JPEG segment length");
        }
        if matches!(
            marker,
            0xc0 | 0xc1
                | 0xc2
                | 0xc3
                | 0xc5
                | 0xc6
                | 0xc7
                | 0xc9
                | 0xca
                | 0xcb
                | 0xcd
                | 0xce
                | 0xcf
        ) {
            if segment_length < 7 {
                bail!("style pack icon contains a truncated JPEG frame header");
            }
            let height = u16::from_be_bytes([bytes[offset + 3], bytes[offset + 4]]) as u32;
            let width = u16::from_be_bytes([bytes[offset + 5], bytes[offset + 6]]) as u32;
            return Ok((width, height));
        }
        if marker == 0xda {
            break;
        }
        offset += segment_length;
    }
    bail!("style pack icon JPEG is missing a supported frame header")
}

fn validate_webp(bytes: &[u8]) -> Result<(u32, u32)> {
    if bytes.len() < 20 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        bail!("style pack icon content is not a WebP image");
    }
    let riff_size = u32::from_le_bytes(bytes[4..8].try_into().expect("WebP RIFF size")) as usize;
    if riff_size.checked_add(8) != Some(bytes.len()) {
        bail!("style pack icon WebP has an invalid RIFF size");
    }
    let chunk = &bytes[12..16];
    let chunk_size =
        u32::from_le_bytes(bytes[16..20].try_into().expect("WebP chunk size")) as usize;
    if chunk_size > bytes.len().saturating_sub(20) {
        bail!("style pack icon WebP contains a truncated image chunk");
    }
    match chunk {
        b"VP8X" => {
            if chunk_size < 10 || bytes.len() < 30 {
                bail!("style pack icon WebP has a truncated VP8X header");
            }
            if bytes[20] & 0x02 != 0 {
                bail!("animated WebP style pack icons are not supported");
            }
            let width = 1 + u32::from_le_bytes([bytes[24], bytes[25], bytes[26], 0]);
            let height = 1 + u32::from_le_bytes([bytes[27], bytes[28], bytes[29], 0]);
            Ok((width, height))
        }
        b"VP8 " => {
            if chunk_size < 10 || bytes.len() < 30 || &bytes[23..26] != b"\x9d\x01\x2a" {
                bail!("style pack icon WebP has an invalid VP8 header");
            }
            let width = u16::from_le_bytes([bytes[26], bytes[27]]) & 0x3fff;
            let height = u16::from_le_bytes([bytes[28], bytes[29]]) & 0x3fff;
            Ok((u32::from(width), u32::from(height)))
        }
        b"VP8L" => {
            if chunk_size < 5 || bytes.len() < 25 || bytes[20] != 0x2f {
                bail!("style pack icon WebP has an invalid VP8L header");
            }
            let width = 1 + u32::from(bytes[21]) + ((u32::from(bytes[22]) & 0x3f) << 8);
            let height = 1
                + (u32::from(bytes[22]) >> 6)
                + (u32::from(bytes[23]) << 2)
                + ((u32::from(bytes[24]) & 0x0f) << 10);
            Ok((width, height))
        }
        _ => bail!("style pack icon WebP has an unsupported first chunk"),
    }
}

pub(super) fn persist_style_pack_icon(
    asset_root: &Path,
    pack_id: &str,
    icon: StylePackIcon,
) -> Result<String> {
    if asset_root.as_os_str().is_empty() {
        bail!("style pack asset root unavailable (memory-only store)");
    }
    let target_dir = asset_root.join(pack_id);
    ensure_dir(&target_dir)?;
    let target_path = target_dir.join(format!("icon.{}", icon.extension));
    if let Err(error) = atomic_write(&target_path, &icon.bytes) {
        let _ = fs::remove_dir_all(&target_dir);
        return Err(error)
            .with_context(|| format!("write style pack icon failed: {}", target_path.display()));
    }
    Ok(target_path.to_string_lossy().into_owned())
}

pub(super) fn cleanup_style_pack_asset_dir(asset_root: &Path, pack_id: &str) {
    if asset_root.as_os_str().is_empty() {
        return;
    }
    let _ = fs::remove_dir_all(asset_root.join(pack_id));
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read};

    use super::{read_stream_bounded, validate_png, StreamBudget, MAX_TOTAL_UNCOMPRESSED_BYTES};

    struct PanicReader;

    impl Read for PanicReader {
        fn read(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
            panic!("declared-size overflow must be rejected before reading")
        }
    }

    #[test]
    fn bounded_reader_rejects_declared_overflow_without_reading() {
        let mut budget = StreamBudget::default();
        let error = read_stream_bounded(&mut PanicReader, "prompt.md", 11, 10, &mut budget)
            .expect_err("declared overflow must fail");

        assert!(format!("{error:#}").contains("prompt.md declared size"));
        assert_eq!(budget.total, 0);
    }

    #[test]
    fn bounded_reader_rejects_streamed_overflow_when_declared_size_is_forged_small() {
        let mut budget = StreamBudget::default();
        let mut reader = Cursor::new(vec![b'x'; 11]);
        let error = read_stream_bounded(&mut reader, "prompt.md", 1, 10, &mut budget)
            .expect_err("streamed overflow must fail");

        assert!(format!("{error:#}").contains("prompt.md streamed size"));
        assert_eq!(budget.total, 0);
    }

    #[test]
    fn bounded_reader_counts_actual_bytes_across_entries() {
        let mut budget = StreamBudget {
            total: MAX_TOTAL_UNCOMPRESSED_BYTES - 4,
        };
        let mut reader = Cursor::new(vec![b'x'; 5]);
        let error = read_stream_bounded(&mut reader, "examples.json", 5, 10, &mut budget)
            .expect_err("actual total overflow must fail");

        assert!(format!("{error:#}").contains("total streamed uncompressed size"));
        assert_eq!(budget.total, MAX_TOTAL_UNCOMPRESSED_BYTES - 4);
    }

    #[test]
    fn png_validator_rejects_a_second_malformed_ihdr_without_panicking() {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        append_png_chunk(&mut png, b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]);
        append_png_chunk(&mut png, b"IHDR", &[]);
        append_png_chunk(&mut png, b"IEND", &[]);

        let error = validate_png(&png).expect_err("a duplicate malformed IHDR must fail");

        assert!(format!("{error:#}").contains("IHDR"));
    }

    fn append_png_chunk(png: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) {
        png.extend_from_slice(&(data.len() as u32).to_be_bytes());
        png.extend_from_slice(chunk_type);
        png.extend_from_slice(data);
        png.extend_from_slice(&[0; 4]);
    }

    #[test]
    fn persist_style_pack_icon_rejects_empty_asset_root() {
        use std::fs;
        use std::path::PathBuf;

        use super::persist_style_pack_icon;
        use super::StylePackIcon;

        let pack_id = "fallback-pack-id";
        let cwd = std::env::current_dir().expect("cwd");
        let relative_dir = cwd.join(pack_id);
        let _ = fs::remove_dir_all(&relative_dir);

        let icon = StylePackIcon {
            extension: "png".to_string(),
            bytes: vec![0x89, 0x50, 0x4e, 0x47],
        };
        let err = persist_style_pack_icon(&PathBuf::new(), pack_id, icon).unwrap_err();
        assert!(
            format!("{err:#}").contains("memory-only store"),
            "{err:#}"
        );
        assert!(
            !relative_dir.exists(),
            "empty asset_root must not create {} under cwd",
            relative_dir.display()
        );
    }
}
