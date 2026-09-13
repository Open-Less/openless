//! Input limits for portable cloud snapshots. Validation never opens client-supplied paths or URLs.

use crate::cloud_sync_types::{CloudSyncPayload, SyncPreferences, SYNC_MAX_REVISION};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::collections::HashSet;

const MAX_DICTIONARY_ENTRIES: usize = 10_000;
const MAX_CORRECTION_RULES: usize = 2_000;
const MAX_STYLE_PACKS: usize = 200;
const MAX_ICON_BYTES: usize = 64 * 1024;

pub(crate) fn validate_payload(payload: &CloudSyncPayload) -> Result<(), &'static str> {
    if payload.dictionary.len() > MAX_DICTIONARY_ENTRIES
        || payload.corrections.len() > MAX_CORRECTION_RULES
        || payload.style_packs.len() > MAX_STYLE_PACKS
    {
        return Err("snapshot contains too many entries");
    }
    unique_ids(payload.dictionary.iter().map(|entry| entry.id.as_str()))?;
    unique_ids(payload.corrections.iter().map(|rule| rule.id.as_str()))?;
    unique_ids(payload.style_packs.iter().map(|pack| pack.id.as_str()))?;

    for entry in &payload.dictionary {
        text(&entry.phrase, 1024, true)?;
        optional_text(entry.note.as_deref(), 4096)?;
        text(&entry.created_at, 64, false)?;
        if entry.hits > SYNC_MAX_REVISION {
            return Err("dictionary hit count exceeds the supported range");
        }
    }
    for rule in &payload.corrections {
        text(&rule.pattern, 4096, true)?;
        text(&rule.replacement, 16 * 1024, false)?;
        text(&rule.created_at, 64, false)?;
    }
    for pack in &payload.style_packs {
        text(&pack.name, 256, true)?;
        text(&pack.description, 8192, false)?;
        optional_text(pack.author.as_deref(), 256)?;
        text(&pack.version, 64, false)?;
        text(&pack.prompt, 200_000, false)?;
        text(&pack.selection_prompt, 200_000, false)?;
        optional_text(pack.created_at.as_deref(), 64)?;
        optional_text(pack.updated_at.as_deref(), 64)?;
        optional_text(pack.recommended_model.as_deref(), 256)?;
        optional_text(pack.compatible_app_version.as_deref(), 64)?;
        optional_text(pack.origin_author_login.as_deref(), 64)?;
        if let Some(origin_id) = &pack.origin_pack_id {
            identifier(origin_id)?;
        }
        if pack.tags.len() > 32 || pack.examples.len() > 50 {
            return Err("style pack contains too many tags or examples");
        }
        for tag in &pack.tags {
            text(tag, 64, true)?;
        }
        for example in &pack.examples {
            optional_text(example.title.as_deref(), 256)?;
            text(&example.input, 8192, false)?;
            text(&example.output, 16 * 1024, false)?;
        }
        if let Some(icon) = &pack.icon_png_base64 {
            validate_icon(icon)?;
        }
    }
    validate_preferences(&payload.preferences)
}

fn validate_preferences(preferences: &SyncPreferences) -> Result<(), &'static str> {
    for id in [
        &preferences.active_style_pack_id,
        &preferences.selection_polish_style_pack_id,
    ]
    .into_iter()
    .flatten()
    {
        identifier(id)?;
    }
    if let Some(modes) = &preferences.enabled_modes {
        if modes.is_empty()
            || modes.len() > 4
            || modes
                .iter()
                .enumerate()
                .any(|(index, mode)| modes[..index].contains(mode))
        {
            return Err("enabled modes must contain one to four distinct modes");
        }
    }
    if let Some(languages) = &preferences.working_languages {
        if languages.len() > 64 {
            return Err("too many working languages");
        }
        for language in languages {
            text(language, 64, true)?;
        }
    }
    if let Some(language) = &preferences.translation_target_language {
        // An empty target uses the app's output-language default.
        text(language, 64, false)?;
    }
    if preferences
        .silence_auto_stop_seconds
        .is_some_and(|seconds| !seconds.is_finite() || !(0.5..=60.0).contains(&seconds))
    {
        return Err("silence auto-stop must be between 0.5 and 60 seconds");
    }
    Ok(())
}

fn unique_ids<'a>(ids: impl Iterator<Item = &'a str>) -> Result<(), &'static str> {
    let mut seen = HashSet::new();
    for id in ids {
        identifier(id)?;
        if !seen.insert(id) {
            return Err("snapshot contains duplicate entry identifiers");
        }
    }
    Ok(())
}

fn identifier(value: &str) -> Result<(), &'static str> {
    if value.is_empty()
        || value.len() > 128
        || value == "."
        || value == ".."
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-:".contains(&byte))
    {
        return Err("invalid entry identifier");
    }
    Ok(())
}

fn text(value: &str, max_bytes: usize, required: bool) -> Result<(), &'static str> {
    if value.len() > max_bytes || value.contains('\0') || (required && value.trim().is_empty()) {
        return Err("text is empty, contains NUL, or exceeds its size limit");
    }
    Ok(())
}

fn optional_text(value: Option<&str>, max_bytes: usize) -> Result<(), &'static str> {
    value.map_or(Ok(()), |value| text(value, max_bytes, false))
}

fn validate_icon(encoded: &str) -> Result<(), &'static str> {
    const INVALID: &str =
        "icon must be a complete static PNG of at most 64 KiB and 1024 by 1024 pixels";
    if encoded.len() > MAX_ICON_BYTES.div_ceil(3) * 4 {
        return Err(INVALID);
    }
    let bytes = STANDARD.decode(encoded).map_err(|_| INVALID)?;
    if bytes.len() > MAX_ICON_BYTES || !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(INVALID);
    }

    // Inspect the bounded PNG container without decoding image pixels on the API worker.
    let mut offset = 8usize;
    let mut has_header = false;
    let mut has_image = false;
    while offset.checked_add(12).is_some_and(|end| end <= bytes.len()) {
        let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().map_err(|_| INVALID)?);
        let length = usize::try_from(length).map_err(|_| INVALID)?;
        let end = offset
            .checked_add(12)
            .and_then(|start| start.checked_add(length))
            .filter(|end| *end <= bytes.len())
            .ok_or(INVALID)?;
        let kind = &bytes[offset + 4..offset + 8];
        if !has_header {
            if kind != b"IHDR" || length != 13 {
                return Err(INVALID);
            }
            let width = u32::from_be_bytes(
                bytes[offset + 8..offset + 12]
                    .try_into()
                    .map_err(|_| INVALID)?,
            );
            let height = u32::from_be_bytes(
                bytes[offset + 12..offset + 16]
                    .try_into()
                    .map_err(|_| INVALID)?,
            );
            if !(1..=1024).contains(&width) || !(1..=1024).contains(&height) {
                return Err(INVALID);
            }
            has_header = true;
        } else if kind == b"IHDR" || kind == b"acTL" {
            return Err(INVALID);
        } else if kind == b"IDAT" {
            has_image = true;
        } else if kind == b"IEND" {
            return if length == 0 && end == bytes.len() && has_image {
                Ok(())
            } else {
                Err(INVALID)
            };
        }
        offset = end;
    }
    Err(INVALID)
}
