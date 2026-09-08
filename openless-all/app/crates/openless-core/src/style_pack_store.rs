//! Shared style-pack repository and lifecycle rules.

use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::errors::{BackendError, BackendErrorCode};
use crate::persistence::{atomic_write, persistence_error, read_or_default};
use crate::shared_types::UserPreferences;
use crate::style_pack_archive::{
    cleanup_style_pack_asset_dir, persist_style_pack_icon, read_style_pack_archive,
    read_style_pack_archive_bytes, ParsedStylePackArchive, StylePackArchiveManifest,
};
use crate::style_packs::{
    builtin_style_pack_for_mode, builtin_style_pack_id, builtin_style_packs,
    default_active_style_pack_id, CustomStylePrompts, StylePack, StylePackExample, StylePackKind,
};
use crate::types::PolishMode;

pub struct StylePackStore {
    path: Option<PathBuf>,
    asset_root: Option<PathBuf>,
    state: Mutex<Vec<StylePack>>,
}

impl StylePackStore {
    pub fn at_data_dir(data_dir: impl AsRef<Path>) -> Result<Self, BackendError> {
        let data_dir = data_dir.as_ref();
        Self::at_paths_internal(
            data_dir.join("style-packs.json"),
            data_dir.join("style-pack-assets"),
            None,
        )
    }

    pub fn at_data_dir_with_preferences(
        data_dir: impl AsRef<Path>,
        preferences: &UserPreferences,
    ) -> Result<Self, BackendError> {
        let data_dir = data_dir.as_ref();
        Self::at_paths_internal(
            data_dir.join("style-packs.json"),
            data_dir.join("style-pack-assets"),
            Some(preferences),
        )
    }

    pub fn at_path(path: PathBuf) -> Result<Self, BackendError> {
        let asset_root = path
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .join("style-pack-assets");
        Self::at_paths(path, asset_root)
    }

    pub fn at_paths(path: PathBuf, asset_root: PathBuf) -> Result<Self, BackendError> {
        Self::at_paths_internal(path, asset_root, None)
    }

    fn at_paths_internal(
        path: PathBuf,
        asset_root: PathBuf,
        preferences: Option<&UserPreferences>,
    ) -> Result<Self, BackendError> {
        let mut packs: Vec<StylePack> = read_or_default(&path)?;
        let mut changed = preferences
            .map(|preferences| migrate_style_packs_from_preferences(&mut packs, preferences))
            .unwrap_or(false);
        changed |= reconcile_builtin_packs(&mut packs) | ensure_at_least_one_enabled(&mut packs);
        sort_packs(&mut packs);
        if changed {
            write_packs(&path, &packs)?;
        }
        Ok(Self {
            path: Some(path),
            asset_root: Some(asset_root),
            state: Mutex::new(packs),
        })
    }

    pub fn in_memory() -> Self {
        let mut packs = builtin_style_packs();
        ensure_at_least_one_enabled(&mut packs);
        Self {
            path: None,
            asset_root: None,
            state: Mutex::new(packs),
        }
    }

    pub fn list(&self) -> Result<Vec<StylePack>, BackendError> {
        Ok(self.lock()?.clone())
    }

    pub fn list_with_active(&self, active_id: &str) -> Result<Vec<StylePack>, BackendError> {
        let mut packs = self.list()?;
        for pack in &mut packs {
            pack.active = pack.id == active_id;
        }
        Ok(packs)
    }

    pub fn get(&self, id: &str) -> Result<StylePack, BackendError> {
        self.lock()?
            .iter()
            .find(|pack| pack.id == id)
            .cloned()
            .ok_or_else(|| not_found(id))
    }

    pub fn get_or_default_active(&self, active_id: &str) -> Result<StylePack, BackendError> {
        let packs = self.lock()?;
        packs
            .iter()
            .find(|pack| pack.id == active_id && pack.enabled)
            .or_else(|| {
                packs
                    .iter()
                    .find(|pack| pack.id == default_active_style_pack_id() && pack.enabled)
            })
            .or_else(|| packs.iter().find(|pack| pack.enabled))
            .cloned()
            .ok_or_else(|| {
                BackendError::new(BackendErrorCode::InvalidState, "no enabled style pack")
            })
    }

    pub fn create(&self, mut pack: StylePack) -> Result<StylePack, BackendError> {
        let mut packs = self.lock()?;
        let requested = if pack.id.trim().is_empty() {
            format!("imported-{}", uuid::Uuid::new_v4().simple())
        } else {
            pack.id.clone()
        };
        pack.id = unique_imported_id(&packs, &requested);
        pack.name = required_text(&pack.name, "style pack name")?;
        pack.kind = StylePackKind::Imported;
        pack.active = false;
        pack.enabled = true;
        let now = chrono::Utc::now().to_rfc3339();
        pack.created_at = Some(now.clone());
        pack.updated_at = Some(now);
        pack.version = normalized_version(&pack.version);
        pack.examples = normalized_examples(pack.examples);
        pack.tags = normalized_tags(&pack.tags);
        packs.push(pack.clone());
        self.persist_locked(&packs)?;
        Ok(pack)
    }

    pub fn update(&self, incoming: StylePack) -> Result<StylePack, BackendError> {
        let mut packs = self.lock()?;
        let slot = packs
            .iter_mut()
            .find(|pack| pack.id == incoming.id)
            .ok_or_else(|| not_found(&incoming.id))?;
        slot.name = required_text(&incoming.name, "style pack name")?;
        slot.description = incoming.description.trim().to_string();
        slot.author = normalized_optional(incoming.author);
        slot.version = normalized_version(&incoming.version);
        slot.selection_prompt = incoming.selection_prompt;
        slot.prompt = incoming.prompt;
        slot.examples = normalized_examples(incoming.examples);
        slot.tags = normalized_tags(&incoming.tags);
        slot.recommended_model = normalized_optional(incoming.recommended_model);
        slot.compatible_app_version = normalized_optional(incoming.compatible_app_version);
        slot.updated_at = Some(chrono::Utc::now().to_rfc3339());
        let updated = slot.clone();
        self.persist_locked(&packs)?;
        Ok(updated)
    }

    pub fn set_origin(
        &self,
        id: &str,
        origin_pack_id: Option<String>,
        origin_author_login: Option<String>,
    ) -> Result<StylePack, BackendError> {
        let mut packs = self.lock()?;
        let slot = packs
            .iter_mut()
            .find(|pack| pack.id == id)
            .ok_or_else(|| not_found(id))?;
        slot.origin_pack_id = normalized_optional(origin_pack_id);
        slot.origin_author_login = normalized_optional(origin_author_login);
        slot.updated_at = Some(chrono::Utc::now().to_rfc3339());
        let updated = slot.clone();
        self.persist_locked(&packs)?;
        Ok(updated)
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<StylePack, BackendError> {
        let mut packs = self.lock()?;
        let index = packs
            .iter()
            .position(|pack| pack.id == id)
            .ok_or_else(|| not_found(id))?;
        packs[index].enabled = enabled;
        packs[index].updated_at = Some(chrono::Utc::now().to_rfc3339());
        ensure_at_least_one_enabled(&mut packs);
        let updated = packs[index].clone();
        self.persist_locked(&packs)?;
        Ok(updated)
    }

    pub fn reset_builtin(&self, id: &str) -> Result<StylePack, BackendError> {
        let mode = builtin_mode(id).ok_or_else(|| {
            BackendError::new(
                BackendErrorCode::InvalidArgument,
                "style pack is not builtin",
            )
        })?;
        let mut packs = self.lock()?;
        let index = packs
            .iter()
            .position(|pack| pack.id == id)
            .ok_or_else(|| not_found(id))?;
        let existing = &packs[index];
        let mut reset = builtin_style_pack_for_mode(mode);
        reset.enabled = existing.enabled;
        reset.created_at = existing.created_at.clone();
        reset.updated_at = Some(chrono::Utc::now().to_rfc3339());
        packs[index] = reset.clone();
        self.persist_locked(&packs)?;
        Ok(reset)
    }

    pub fn remove_imported(&self, id: &str) -> Result<(), BackendError> {
        let mut packs = self.lock()?;
        let index = packs
            .iter()
            .position(|pack| pack.id == id)
            .ok_or_else(|| not_found(id))?;
        if packs[index].kind == StylePackKind::Builtin {
            return Err(BackendError::new(
                BackendErrorCode::InvalidArgument,
                "builtin style pack cannot be deleted",
            ));
        }
        let removed = packs.remove(index);
        ensure_at_least_one_enabled(&mut packs);
        self.persist_locked(&packs)?;
        if let Some(asset_root) = &self.asset_root {
            cleanup_style_pack_asset_dir(asset_root, &removed.id);
        }
        Ok(())
    }

    pub fn import_from_zip(&self, path: &Path) -> Result<StylePack, BackendError> {
        let parsed = read_style_pack_archive(path).map_err(archive_error)?;
        self.import_parsed_archive(parsed)
    }

    pub fn import_from_zip_bytes(&self, bytes: &[u8]) -> Result<StylePack, BackendError> {
        let parsed = read_style_pack_archive_bytes(bytes).map_err(archive_error)?;
        self.import_parsed_archive(parsed)
    }

    /// Imports a validated Marketplace archive while committing its remote
    /// origin in the same persisted style-pack document. Callers never observe
    /// an imported pack without the origin required for later supersede/fork
    /// decisions.
    pub fn import_from_zip_bytes_with_origin(
        &self,
        bytes: &[u8],
        origin_pack_id: String,
        origin_author_login: Option<String>,
    ) -> Result<StylePack, BackendError> {
        let mut parsed = read_style_pack_archive_bytes(bytes).map_err(archive_error)?;
        parsed.manifest.origin_pack_id = Some(origin_pack_id);
        parsed.manifest.origin_author_login = origin_author_login;
        self.import_parsed_archive(parsed)
    }

    fn import_parsed_archive(
        &self,
        parsed: ParsedStylePackArchive,
    ) -> Result<StylePack, BackendError> {
        let manifest = parsed.manifest;
        let mut packs = self.lock()?;
        let pack_id = unique_imported_id(&packs, &manifest.id);
        let icon_path = match (parsed.icon, self.asset_root.as_deref()) {
            (Some(icon), Some(asset_root)) => {
                Some(persist_style_pack_icon(asset_root, &pack_id, icon).map_err(archive_error)?)
            }
            (Some(_), None) => {
                return Err(BackendError::new(
                    BackendErrorCode::Persistence,
                    "style pack asset storage is unavailable",
                ));
            }
            (None, _) => None,
        };
        let now = chrono::Utc::now().to_rfc3339();
        let pack = StylePack {
            id: pack_id,
            name: required_text(&manifest.name, "style pack name")?,
            description: manifest.description.trim().to_string(),
            author: normalized_optional(manifest.author),
            version: normalized_version(&manifest.version),
            kind: StylePackKind::Imported,
            base_mode: manifest.base_mode,
            selection_prompt: manifest.selection_prompt.unwrap_or_default(),
            prompt: parsed.prompt,
            examples: normalized_examples(parsed.examples),
            tags: normalized_tags(&manifest.tags),
            icon_path,
            created_at: Some(now.clone()),
            updated_at: Some(now),
            enabled: true,
            active: false,
            recommended_model: normalized_optional(manifest.recommended_model),
            compatible_app_version: normalized_optional(manifest.compatible_app_version),
            origin_pack_id: normalized_optional(manifest.origin_pack_id),
            origin_author_login: normalized_optional(manifest.origin_author_login),
        };
        let mut next = packs.clone();
        next.insert(0, pack.clone());
        if let Err(error) = self.persist_locked(&next) {
            if pack.icon_path.is_some() {
                if let Some(asset_root) = &self.asset_root {
                    cleanup_style_pack_asset_dir(asset_root, &pack.id);
                }
            }
            return Err(error);
        }
        *packs = next;
        Ok(pack)
    }

    pub fn export_zip_bytes(&self, id: &str) -> Result<Vec<u8>, BackendError> {
        let pack = self.get(id)?;
        let cursor = Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        let icon_file = pack
            .icon_path
            .as_deref()
            .and_then(|path| Path::new(path).file_name())
            .and_then(|name| name.to_str())
            .map(|name| format!("assets/{name}"));
        let manifest = StylePackArchiveManifest {
            schema_version: 1,
            id: pack.id.clone(),
            name: pack.name.clone(),
            description: pack.description.clone(),
            author: pack.author.clone(),
            version: pack.version.clone(),
            base_mode: pack.base_mode,
            selection_prompt: (!pack.selection_prompt.trim().is_empty())
                .then(|| pack.selection_prompt.clone()),
            tags: pack.tags.clone(),
            prompt_file: "prompt.md".into(),
            examples_file: "examples.json".into(),
            icon_file: icon_file.clone(),
            recommended_model: pack.recommended_model.clone(),
            compatible_app_version: pack.compatible_app_version.clone(),
            origin_pack_id: pack.origin_pack_id.clone(),
            origin_author_login: pack.origin_author_login.clone(),
        };

        zip.start_file("manifest.json", options)
            .map_err(archive_error)?;
        zip.write_all(
            serde_json::to_string_pretty(&manifest)
                .map_err(|_| persistence_error("encode style pack manifest"))?
                .as_bytes(),
        )
        .map_err(|_| persistence_error("write style pack manifest"))?;
        zip.start_file("prompt.md", options)
            .map_err(archive_error)?;
        zip.write_all(pack.prompt.as_bytes())
            .map_err(|_| persistence_error("write style pack prompt"))?;
        zip.start_file("examples.json", options)
            .map_err(archive_error)?;
        zip.write_all(
            serde_json::to_string_pretty(&pack.examples)
                .map_err(|_| persistence_error("encode style pack examples"))?
                .as_bytes(),
        )
        .map_err(|_| persistence_error("write style pack examples"))?;

        if let (Some(source_path), Some(entry_name)) = (&pack.icon_path, &icon_file) {
            let source_path = Path::new(source_path);
            if source_path.is_file() {
                zip.start_file(entry_name, options).map_err(archive_error)?;
                let icon =
                    fs::read(source_path).map_err(|_| persistence_error("read style pack icon"))?;
                zip.write_all(&icon)
                    .map_err(|_| persistence_error("write style pack icon"))?;
            }
        }
        let cursor = zip.finish().map_err(archive_error)?;
        Ok(cursor.into_inner())
    }

    pub fn export_to_zip(&self, id: &str, target: &Path) -> Result<(), BackendError> {
        atomic_write(target, &self.export_zip_bytes(id)?)
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Vec<StylePack>>, BackendError> {
        self.state.lock().map_err(|_| {
            BackendError::new(BackendErrorCode::Internal, "style pack store lock poisoned")
        })
    }

    fn persist_locked(&self, packs: &[StylePack]) -> Result<(), BackendError> {
        match &self.path {
            Some(path) => write_packs(path, packs),
            None => Ok(()),
        }
    }
}

pub fn migrate_style_packs_from_preferences(
    packs: &mut Vec<StylePack>,
    preferences: &UserPreferences,
) -> bool {
    let mut changed = false;
    let legacy_prompts = preferences.style_system_prompts.clone();
    for builtin in builtin_style_packs() {
        if let Some(index) = packs.iter().position(|pack| pack.id == builtin.id) {
            let pack = &mut packs[index];
            if pack.kind != StylePackKind::Builtin {
                pack.kind = StylePackKind::Builtin;
                changed = true;
            }
            if pack.name.trim().is_empty() {
                pack.name = builtin.name.clone();
                changed = true;
            }
            if pack.description.trim().is_empty() {
                pack.description = builtin.description.clone();
                changed = true;
            }
            if pack.prompt.trim().is_empty() {
                pack.prompt = builtin.prompt.clone();
                changed = true;
            }
            if pack.selection_prompt.trim().is_empty() {
                pack.selection_prompt = builtin.selection_prompt.clone();
                changed = true;
            }
            if pack.examples.is_empty() {
                pack.examples = builtin.examples.clone();
                changed = true;
            }
            if pack.tags.is_empty() {
                pack.tags = builtin.tags.clone();
                changed = true;
            }
            if pack.version.trim().is_empty() {
                pack.version = builtin.version.clone();
                changed = true;
            }
            if pack.author.is_none() {
                pack.author = builtin.author.clone();
                changed = true;
            }
            if pack.compatible_app_version.is_none() {
                pack.compatible_app_version = builtin.compatible_app_version.clone();
                changed = true;
            }
            if pack.created_at.is_none() {
                pack.created_at = Some(chrono::Utc::now().to_rfc3339());
                changed = true;
            }
            if pack.base_mode != builtin.base_mode {
                pack.base_mode = builtin.base_mode;
                changed = true;
            }
        } else {
            let mut pack = builtin;
            pack.prompt = legacy_prompts.for_mode(pack.base_mode).to_string();
            pack.enabled = preferences.enabled_modes.contains(&pack.base_mode);
            let now = chrono::Utc::now().to_rfc3339();
            pack.created_at = Some(now.clone());
            pack.updated_at = Some(now);
            packs.push(pack);
            changed = true;
        }
    }
    sort_packs(packs);
    changed
}

fn write_packs(path: &Path, packs: &[StylePack]) -> Result<(), BackendError> {
    let bytes =
        serde_json::to_vec_pretty(packs).map_err(|_| persistence_error("encode style packs"))?;
    atomic_write(path, &bytes)
}

fn reconcile_builtin_packs(packs: &mut Vec<StylePack>) -> bool {
    let mut changed = false;
    for builtin in builtin_style_packs() {
        if let Some(local) = packs.iter_mut().find(|pack| pack.id == builtin.id) {
            if version_newer(&builtin.version, &local.version) {
                local.version = builtin.version;
                local.prompt = builtin.prompt;
                local.updated_at = Some(chrono::Utc::now().to_rfc3339());
                changed = true;
            }
        } else {
            packs.push(builtin);
            changed = true;
        }
    }
    changed
}

fn ensure_at_least_one_enabled(packs: &mut [StylePack]) -> bool {
    if packs.iter().any(|pack| pack.enabled) {
        return false;
    }
    let index = packs
        .iter()
        .position(|pack| pack.id == default_active_style_pack_id())
        .or_else(|| (!packs.is_empty()).then_some(0));
    if let Some(pack) = index.and_then(|index| packs.get_mut(index)) {
        pack.enabled = true;
        pack.updated_at = Some(chrono::Utc::now().to_rfc3339());
        return true;
    }
    false
}

fn sort_packs(packs: &mut [StylePack]) {
    packs.sort_by(|left, right| {
        let kind = |pack: &StylePack| match pack.kind {
            StylePackKind::Builtin => 0,
            StylePackKind::Imported => 1,
        };
        let mode = |pack: &StylePack| match pack.base_mode {
            PolishMode::Raw => 0,
            PolishMode::Light => 1,
            PolishMode::Structured => 2,
            PolishMode::Formal => 3,
        };
        (kind(left), mode(left), &left.name).cmp(&(kind(right), mode(right), &right.name))
    });
}

fn builtin_mode(id: &str) -> Option<PolishMode> {
    [
        PolishMode::Raw,
        PolishMode::Light,
        PolishMode::Structured,
        PolishMode::Formal,
    ]
    .into_iter()
    .find(|mode| builtin_style_pack_id(*mode) == id)
}

fn version_newer(left: &str, right: &str) -> bool {
    let parts = |value: &str| {
        value
            .split('-')
            .next()
            .unwrap_or(value)
            .split('.')
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    let left = parts(left);
    let right = parts(right);
    (0..left.len().max(right.len()))
        .find_map(|index| {
            let left = left.get(index).copied().unwrap_or(0);
            let right = right.get(index).copied().unwrap_or(0);
            (left != right).then_some(left > right)
        })
        .unwrap_or(false)
}

pub fn sync_style_pack_preferences(preferences: &mut UserPreferences, packs: &[StylePack]) -> bool {
    let enabled = packs.iter().filter(|pack| pack.enabled).collect::<Vec<_>>();
    let active = packs
        .iter()
        .find(|pack| pack.id == preferences.active_style_pack_id && pack.enabled)
        .or_else(|| {
            packs.iter().find(|pack| {
                pack.id == builtin_style_pack_id(preferences.default_mode) && pack.enabled
            })
        })
        .or_else(|| enabled.first().copied());

    let Some(active_pack) = active else {
        return false;
    };

    let mut changed = false;
    if preferences.active_style_pack_id != active_pack.id {
        preferences.active_style_pack_id = active_pack.id.clone();
        changed = true;
    }
    if preferences.default_mode != active_pack.base_mode {
        preferences.default_mode = active_pack.base_mode;
        changed = true;
    }
    if !packs
        .iter()
        .any(|pack| pack.id == preferences.selection_polish_style_pack_id && pack.enabled)
    {
        preferences.selection_polish_style_pack_id = active_pack.id.clone();
        changed = true;
    }

    let enabled_modes = enabled_modes_from_style_packs(packs);
    if preferences.enabled_modes != enabled_modes {
        preferences.enabled_modes = enabled_modes;
        changed = true;
    }
    changed | sync_builtin_style_prompt_preferences(preferences, packs)
}

fn sync_builtin_style_prompt_preferences(
    preferences: &mut UserPreferences,
    packs: &[StylePack],
) -> bool {
    let mut changed = false;
    let mut saw_builtin = false;
    for mode in [
        PolishMode::Raw,
        PolishMode::Light,
        PolishMode::Structured,
        PolishMode::Formal,
    ] {
        let Some(pack) = packs
            .iter()
            .find(|pack| pack.kind == StylePackKind::Builtin && pack.base_mode == mode)
        else {
            continue;
        };
        saw_builtin = true;
        if preferences.style_system_prompts.for_mode(mode) == pack.prompt {
            continue;
        }
        match mode {
            PolishMode::Raw => preferences.style_system_prompts.raw = pack.prompt.clone(),
            PolishMode::Light => preferences.style_system_prompts.light = pack.prompt.clone(),
            PolishMode::Structured => {
                preferences.style_system_prompts.structured = pack.prompt.clone()
            }
            PolishMode::Formal => preferences.style_system_prompts.formal = pack.prompt.clone(),
        }
        changed = true;
    }
    if saw_builtin && preferences.custom_style_prompts != CustomStylePrompts::default() {
        preferences.custom_style_prompts = CustomStylePrompts::default();
        changed = true;
    }
    changed
}

pub fn enabled_modes_from_style_packs(packs: &[StylePack]) -> Vec<PolishMode> {
    [
        PolishMode::Raw,
        PolishMode::Light,
        PolishMode::Structured,
        PolishMode::Formal,
    ]
    .into_iter()
    .filter(|mode| {
        packs
            .iter()
            .any(|pack| pack.enabled && pack.base_mode == *mode)
    })
    .collect()
}

fn normalized_examples(examples: Vec<StylePackExample>) -> Vec<StylePackExample> {
    examples
        .into_iter()
        .filter_map(|example| {
            let input = example.input.trim().to_string();
            let output = example.output.trim().to_string();
            (!input.is_empty() || !output.is_empty()).then_some(StylePackExample {
                title: normalized_optional(example.title),
                input,
                output,
            })
        })
        .collect()
}

fn normalized_tags(tags: &[String]) -> Vec<String> {
    let mut output = Vec::new();
    for tag in tags {
        let tag = tag.trim();
        if !tag.is_empty() && !output.iter().any(|existing| existing == tag) {
            output.push(tag.to_string());
        }
    }
    output
}

fn normalized_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim();
        (!value.is_empty()).then(|| value.to_string())
    })
}

fn normalized_version(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        "1.0.0".to_string()
    } else {
        value.to_string()
    }
}

fn required_text(value: &str, field: &str) -> Result<String, BackendError> {
    let value = value.trim();
    if value.is_empty() {
        Err(BackendError::new(
            BackendErrorCode::InvalidArgument,
            format!("{field} is empty"),
        ))
    } else {
        Ok(value.to_string())
    }
}

fn unique_imported_id(packs: &[StylePack], requested: &str) -> String {
    let mut base = requested
        .trim()
        .chars()
        .filter_map(|character| match character {
            character if character.is_ascii_alphanumeric() => Some(character.to_ascii_lowercase()),
            '-' | '_' | '.' => Some(character),
            ' ' | '/' | '\\' => Some('-'),
            _ => None,
        })
        .collect::<String>();
    base = base.trim_matches(['-', '.', '_']).to_string();
    if base.is_empty() {
        base = format!("imported-{}", uuid::Uuid::new_v4().simple());
    } else if base.starts_with("builtin.") {
        base = format!("imported.{base}");
    }
    if !packs.iter().any(|pack| pack.id == base) {
        return base;
    }
    for index in 2usize.. {
        let candidate = format!("{base}-{index}");
        if !packs.iter().any(|pack| pack.id == candidate) {
            return candidate;
        }
    }
    unreachable!()
}

fn not_found(id: &str) -> BackendError {
    BackendError::new(
        BackendErrorCode::InvalidArgument,
        format!("style pack {id} not found"),
    )
}

fn archive_error(error: impl std::fmt::Display) -> BackendError {
    BackendError::new(
        BackendErrorCode::InvalidArgument,
        format!("invalid style pack archive: {error}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_preserves_builtin_invariants_and_normalizes_imports() {
        let path = std::env::temp_dir().join(format!(
            "openless-core-style-packs-{}.json",
            uuid::Uuid::new_v4().simple()
        ));
        let store = StylePackStore::at_path(path.clone()).unwrap();
        assert_eq!(store.list().unwrap().len(), 4);
        let mut imported = StylePack {
            id: "Builtin.Custom Pack".to_string(),
            name: "  My Pack  ".to_string(),
            prompt: "prompt".to_string(),
            tags: vec![" tag ".to_string(), "tag".to_string()],
            ..StylePack::default()
        };
        imported = store.create(imported).unwrap();
        assert_eq!(imported.id, "imported.builtin.custom-pack");
        assert_eq!(imported.name, "My Pack");
        assert_eq!(imported.tags, vec!["tag"]);
        store.remove_imported(&imported.id).unwrap();
        assert_eq!(
            store
                .remove_imported(&default_active_style_pack_id())
                .unwrap_err()
                .code,
            BackendErrorCode::InvalidArgument
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn disabling_every_pack_reenables_the_product_default() {
        let store = StylePackStore::in_memory();
        for pack in store.list().unwrap() {
            store.set_enabled(&pack.id, false).unwrap();
        }
        assert!(store.get(&default_active_style_pack_id()).unwrap().enabled);
    }
}

#[cfg(test)]
#[path = "style_pack_store_tests.rs"]
mod contract_tests;
