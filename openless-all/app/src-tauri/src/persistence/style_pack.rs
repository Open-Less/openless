#![cfg_attr(target_os = "linux", allow(dead_code, unused_variables))]
//! Style-pack store: the on-disk pack list + asset directory, ZIP import/export,
//! and the reconciliation logic that keeps `UserPreferences` in sync with the
//! enabled packs.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{atomic_write, data_dir, ensure_dir, read_or_default, PreferencesStore};
use crate::types::{
    builtin_style_pack_for_mode, builtin_style_pack_id, builtin_style_packs,
    default_active_style_pack_id, CustomStylePrompts, PolishMode, StylePack, StylePackExample,
    StylePackKind, UserPreferences, BUILTIN_STYLE_PACK_LIGHT_ID,
};

const STYLE_PACKS_FILE: &str = "style-packs.json";
const STYLE_PACK_ASSETS_DIR: &str = "style-pack-assets";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StylePackArchiveManifest {
    schema_version: u32,
    id: String,
    name: String,
    description: String,
    author: Option<String>,
    version: String,
    base_mode: PolishMode,
    tags: Vec<String>,
    prompt_file: String,
    examples_file: String,
    icon_file: Option<String>,
    recommended_model: Option<String>,
    compatible_app_version: Option<String>,
    /// Marketplace 上游关系。旧 ZIP 没有此字段时自动为 None；
    /// 兼容早期口误/拼写包里可能出现的 `orion*` 字段名。
    #[serde(
        default,
        alias = "orionPackId",
        alias = "orion_pack_id",
        alias = "origin_pack_id"
    )]
    origin_pack_id: Option<String>,
    #[serde(
        default,
        alias = "orionAuthorLogin",
        alias = "orion_author_login",
        alias = "origin_author_login"
    )]
    origin_author_login: Option<String>,
}

pub struct StylePackStore {
    path: PathBuf,
    asset_root: PathBuf,
    state: Mutex<Vec<StylePack>>,
}

impl StylePackStore {
    pub fn new(prefs: &PreferencesStore) -> Result<Self> {
        let dir = data_dir()?;
        ensure_dir(&dir)?;
        let path = dir.join(STYLE_PACKS_FILE);
        let asset_root = dir.join(STYLE_PACK_ASSETS_DIR);
        ensure_dir(&asset_root)?;

        let mut packs = if path.exists() {
            read_or_default::<Vec<StylePack>>(&path).unwrap_or_else(|error| {
                log::warn!(
                    "[style-packs] load {} failed, using builtin defaults: {}",
                    path.display(),
                    error
                );
                Vec::new()
            })
        } else {
            Vec::new()
        };

        let mut prefs_snapshot = prefs.get();
        let mut changed = migrate_style_packs_from_preferences(&mut packs, &prefs_snapshot);
        if ensure_at_least_one_style_pack_enabled(&mut packs) {
            changed = true;
        }
        let active_pref_for_log = prefs_snapshot.active_style_pack_id.clone();
        let enabled_modes_for_log = prefs_snapshot.enabled_modes.clone();
        if sync_style_pack_preferences(&mut prefs_snapshot, &packs) {
            prefs.set(prefs_snapshot)?;
        }
        if changed {
            write_style_packs_file(&path, &packs)?;
        }
        log::info!(
            "[style-pack] store ready: file={} packs={} changed={} active_pref={} enabled_modes={:?}",
            path.display(),
            packs.len(),
            changed,
            active_pref_for_log,
            enabled_modes_for_log
        );

        Ok(Self {
            path,
            asset_root,
            state: Mutex::new(packs),
        })
    }

    /// 降级实例：data_dir 不可用时使用临时路径和空列表，写操作会安静地失败。
    pub(crate) fn new_fallback() -> Self {
        let tmp = std::env::temp_dir();
        Self {
            path: tmp.join("openless_style_packs_fallback.json"),
            asset_root: tmp.join("openless_style_pack_assets_fallback"),
            state: Mutex::new(Vec::new()),
        }
    }

    pub fn list(&self) -> Result<Vec<StylePack>> {
        Ok(self.state.lock().clone())
    }

    pub fn list_with_active(&self, active_style_pack_id: &str) -> Result<Vec<StylePack>> {
        let mut packs = self.list()?;
        for pack in &mut packs {
            pack.active = pack.id == active_style_pack_id;
        }
        Ok(packs)
    }

    pub fn get(&self, id: &str) -> Result<StylePack> {
        self.state
            .lock()
            .iter()
            .find(|pack| pack.id == id)
            .cloned()
            .ok_or_else(|| anyhow!("style pack {} not found", id))
    }

    pub fn get_or_default_active(&self, active_style_pack_id: &str) -> Result<StylePack> {
        let packs = self.state.lock().clone();
        if let Some(pack) = packs
            .iter()
            .find(|pack| pack.id == active_style_pack_id && pack.enabled)
            .cloned()
        {
            return Ok(pack);
        }
        if let Some(pack) = packs
            .iter()
            .find(|pack| pack.id == BUILTIN_STYLE_PACK_LIGHT_ID && pack.enabled)
            .cloned()
        {
            return Ok(pack);
        }
        packs
            .into_iter()
            .find(|pack| pack.enabled)
            .ok_or_else(|| anyhow!("no enabled style pack available"))
    }

    /// 从模板新建一个 imported 风格包（"+"按钮路径）。
    /// 跟 ZIP 导入不同：没有 manifest.json、没有 assets，纯空白模板。
    /// 调用方负责 set `prefs.active_style_pack_id` 等高层 wiring（这里只管落盘）。
    pub fn create_from_template(&self, template: StylePack) -> Result<StylePack> {
        let mut packs = self.state.lock();
        let base_id = if template.id.trim().is_empty() {
            format!("imported-{}", Uuid::new_v4().simple())
        } else {
            template.id.clone()
        };
        let assigned_id = unique_imported_style_pack_id(&packs, &base_id);
        let now = Utc::now().to_rfc3339();
        let mut pack = template;
        pack.id = assigned_id;
        pack.kind = StylePackKind::Imported;
        pack.created_at = Some(now.clone());
        pack.updated_at = Some(now);
        pack.active = false;
        pack.enabled = true;
        packs.push(pack.clone());
        write_style_packs_file(&self.path, &packs)?;
        log::info!(
            "[style-pack] created from template id={} base_mode={:?} prompt_chars={} examples={}",
            pack.id,
            pack.base_mode,
            pack.prompt.chars().count(),
            pack.examples.len()
        );
        Ok(pack)
    }

    pub fn upsert(&self, incoming: StylePack) -> Result<StylePack> {
        let mut packs = self.state.lock();
        let index = packs
            .iter()
            .position(|pack| pack.id == incoming.id)
            .ok_or_else(|| anyhow!("style pack {} not found", incoming.id))?;
        let existing = packs[index].clone();
        let updated = merge_style_pack_update(existing, incoming)?;
        packs[index] = updated.clone();
        write_style_packs_file(&self.path, &packs)?;
        log::info!(
            "[style-pack] saved id={} kind={:?} base_mode={:?} prompt_chars={} examples={} tags={} version={}",
            updated.id,
            updated.kind,
            updated.base_mode,
            updated.prompt.chars().count(),
            updated.examples.len(),
            updated.tags.len(),
            updated.version
        );
        Ok(updated)
    }

    /// 设置衍生关系；marketplace_install 安装本地包后绑定 upstream id + author。
    /// 单独走这里是为了不让前端通用 save 路径误清这两字段。
    pub fn set_origin(
        &self,
        id: &str,
        origin_pack_id: Option<String>,
        origin_author_login: Option<String>,
    ) -> Result<StylePack> {
        let mut packs = self.state.lock();
        let index = packs
            .iter()
            .position(|pack| pack.id == id)
            .ok_or_else(|| anyhow!("style pack {} not found", id))?;
        packs[index].origin_pack_id = normalize_optional_text(origin_pack_id);
        packs[index].origin_author_login = normalize_optional_text(origin_author_login);
        packs[index].updated_at = Some(Utc::now().to_rfc3339());
        let updated = packs[index].clone();
        write_style_packs_file(&self.path, &packs)?;
        Ok(updated)
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<StylePack> {
        let mut packs = self.state.lock();
        let index = packs
            .iter()
            .position(|pack| pack.id == id)
            .ok_or_else(|| anyhow!("style pack {} not found", id))?;
        packs[index].enabled = enabled;
        packs[index].updated_at = Some(Utc::now().to_rfc3339());
        if ensure_at_least_one_style_pack_enabled(&mut packs) {
            packs[index].updated_at = Some(Utc::now().to_rfc3339());
        }
        let updated = packs[index].clone();
        write_style_packs_file(&self.path, &packs)?;
        log::info!(
            "[style-pack] set_enabled id={} enabled={} base_mode={:?}",
            updated.id,
            updated.enabled,
            updated.base_mode
        );
        Ok(updated)
    }

    pub fn reset_builtin(&self, id: &str) -> Result<StylePack> {
        let mode = builtin_mode_from_style_pack_id(id)
            .ok_or_else(|| anyhow!("style pack {} is not a builtin pack", id))?;
        let mut packs = self.state.lock();
        let index = packs
            .iter()
            .position(|pack| pack.id == id)
            .ok_or_else(|| anyhow!("style pack {} not found", id))?;
        let existing = packs[index].clone();
        let mut reset = builtin_style_pack_for_mode(mode);
        reset.enabled = existing.enabled;
        reset.created_at = existing
            .created_at
            .or_else(|| Some(Utc::now().to_rfc3339()));
        reset.updated_at = Some(Utc::now().to_rfc3339());
        packs[index] = reset.clone();
        write_style_packs_file(&self.path, &packs)?;
        log::info!(
            "[style-pack] reset_builtin id={} base_mode={:?} prompt_chars={} examples={}",
            reset.id,
            reset.base_mode,
            reset.prompt.chars().count(),
            reset.examples.len()
        );
        Ok(reset)
    }

    pub fn remove_imported(&self, id: &str) -> Result<()> {
        let mut packs = self.state.lock();
        let index = packs
            .iter()
            .position(|pack| pack.id == id)
            .ok_or_else(|| anyhow!("style pack {} not found", id))?;
        if packs[index].kind == StylePackKind::Builtin {
            return Err(anyhow!("builtin style pack cannot be deleted"));
        }
        let removed = packs[index].clone();
        remove_style_pack_assets(&self.asset_root, &packs[index]);
        packs.remove(index);
        if ensure_at_least_one_style_pack_enabled(&mut packs) {
            // write updated fallback state as well
        }
        write_style_packs_file(&self.path, &packs)?;
        log::info!(
            "[style-pack] removed imported id={} base_mode={:?}",
            removed.id,
            removed.base_mode
        );
        Ok(())
    }

    pub fn import_from_zip(&self, zip_path: &Path) -> Result<StylePack> {
        let file = fs::File::open(zip_path)
            .with_context(|| format!("open style pack zip failed: {}", zip_path.display()))?;
        let mut archive = zip::ZipArchive::new(file).context("open style pack zip archive")?;
        let manifest: StylePackArchiveManifest =
            read_zip_json_entry(&mut archive, "manifest.json")?;
        let prompt = read_zip_string_entry(&mut archive, &manifest.prompt_file)?;
        let examples =
            read_zip_json_entry::<Vec<StylePackExample>>(&mut archive, &manifest.examples_file)?;

        let mut packs = self.state.lock();
        let now = Utc::now().to_rfc3339();
        let pack_id = unique_imported_style_pack_id(&packs, &manifest.id);
        let icon_path = if let Some(icon_file) = manifest.icon_file.as_deref() {
            extract_style_pack_icon(&mut archive, &self.asset_root, &pack_id, icon_file)?
        } else {
            None
        };
        let pack = StylePack {
            id: pack_id,
            name: manifest.name.trim().to_string(),
            description: manifest.description.trim().to_string(),
            author: manifest
                .author
                .and_then(|value| normalize_optional_text(Some(value))),
            version: normalize_version(&manifest.version),
            kind: StylePackKind::Imported,
            base_mode: manifest.base_mode,
            prompt,
            examples,
            tags: normalize_tags(&manifest.tags),
            icon_path,
            created_at: Some(now.clone()),
            updated_at: Some(now),
            enabled: true,
            active: false,
            recommended_model: manifest
                .recommended_model
                .and_then(|value| normalize_optional_text(Some(value))),
            compatible_app_version: manifest
                .compatible_app_version
                .and_then(|value| normalize_optional_text(Some(value))),
            origin_pack_id: normalize_optional_text(manifest.origin_pack_id),
            origin_author_login: normalize_optional_text(manifest.origin_author_login),
        };
        packs.insert(0, pack.clone());
        write_style_packs_file(&self.path, &packs)?;
        log::info!(
            "[style-pack] imported source={} installed_id={} manifest_id={} base_mode={:?} prompt_chars={} examples={} tags={} icon={}",
            zip_path.display(),
            pack.id,
            manifest.id,
            pack.base_mode,
            pack.prompt.chars().count(),
            pack.examples.len(),
            pack.tags.len(),
            pack.icon_path.is_some()
        );
        Ok(pack)
    }

    pub fn export_to_zip(&self, id: &str, target_path: &Path) -> Result<()> {
        let pack = self.get(id)?;
        if let Some(parent) = target_path.parent() {
            ensure_dir(parent)?;
        }
        let file = fs::File::create(target_path)
            .with_context(|| format!("create style pack zip failed: {}", target_path.display()))?;
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        let icon_file = pack
            .icon_path
            .as_deref()
            .and_then(|path| Path::new(path).file_name())
            .and_then(|file_name| file_name.to_str())
            .map(|name| format!("assets/{name}"));

        let manifest = StylePackArchiveManifest {
            schema_version: 1,
            id: pack.id.clone(),
            name: pack.name.clone(),
            description: pack.description.clone(),
            author: pack.author.clone(),
            version: pack.version.clone(),
            base_mode: pack.base_mode,
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
            .context("write style pack manifest entry")?;
        zip.write_all(
            serde_json::to_string_pretty(&manifest)
                .context("encode style pack manifest")?
                .as_bytes(),
        )
        .context("write style pack manifest body")?;

        zip.start_file("prompt.md", options)
            .context("write style pack prompt entry")?;
        zip.write_all(pack.prompt.as_bytes())
            .context("write style pack prompt body")?;

        zip.start_file("examples.json", options)
            .context("write style pack examples entry")?;
        zip.write_all(
            serde_json::to_string_pretty(&pack.examples)
                .context("encode style pack examples")?
                .as_bytes(),
        )
        .context("write style pack examples body")?;

        if let (Some(source_icon_path), Some(zip_icon_path)) = (&pack.icon_path, &icon_file) {
            let icon_source = Path::new(source_icon_path);
            if icon_source.exists() {
                zip.start_file(zip_icon_path, options)
                    .context("write style pack icon entry")?;
                let bytes = fs::read(icon_source).with_context(|| {
                    format!("read style pack icon failed: {}", icon_source.display())
                })?;
                zip.write_all(&bytes)
                    .context("write style pack icon body")?;
            }
        }

        zip.finish().context("finalize style pack zip")?;
        log::info!(
            "[style-pack] exported id={} target={} base_mode={:?} prompt_chars={} examples={} icon={}",
            pack.id,
            target_path.display(),
            pack.base_mode,
            pack.prompt.chars().count(),
            pack.examples.len(),
            pack.icon_path.is_some()
        );
        Ok(())
    }
}

fn write_style_packs_file(path: &Path, packs: &[StylePack]) -> Result<()> {
    let json = serde_json::to_vec_pretty(packs).context("encode style packs failed")?;
    atomic_write(path, &json)
}

fn migrate_style_packs_from_preferences(
    packs: &mut Vec<StylePack>,
    prefs: &UserPreferences,
) -> bool {
    let mut changed = false;
    let legacy_prompts = prefs.style_system_prompts.clone();
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
                pack.created_at = Some(Utc::now().to_rfc3339());
                changed = true;
            }
            if pack.base_mode != builtin.base_mode {
                pack.base_mode = builtin.base_mode;
                changed = true;
            }
        } else {
            let mut pack = builtin.clone();
            pack.prompt = legacy_prompts.for_mode(pack.base_mode).to_string();
            pack.enabled = prefs.enabled_modes.contains(&pack.base_mode);
            pack.created_at = Some(Utc::now().to_rfc3339());
            pack.updated_at = Some(Utc::now().to_rfc3339());
            packs.push(pack);
            changed = true;
        }
    }
    packs.sort_by(|left, right| {
        style_pack_sort_key(left)
            .cmp(&style_pack_sort_key(right))
            .then_with(|| left.name.cmp(&right.name))
    });
    changed
}

fn style_pack_sort_key(pack: &StylePack) -> (u8, u8) {
    let kind_rank = match pack.kind {
        StylePackKind::Builtin => 0,
        StylePackKind::Imported => 1,
    };
    let mode_rank = match pack.base_mode {
        PolishMode::Raw => 0,
        PolishMode::Light => 1,
        PolishMode::Structured => 2,
        PolishMode::Formal => 3,
    };
    (kind_rank, mode_rank)
}

fn ensure_at_least_one_style_pack_enabled(packs: &mut [StylePack]) -> bool {
    if packs.iter().any(|pack| pack.enabled) {
        return false;
    }
    if let Some(pack) = packs
        .iter_mut()
        .find(|pack| pack.id == default_active_style_pack_id())
    {
        pack.enabled = true;
        pack.updated_at = Some(Utc::now().to_rfc3339());
        return true;
    }
    if let Some(first) = packs.first_mut() {
        first.enabled = true;
        first.updated_at = Some(Utc::now().to_rfc3339());
        return true;
    }
    false
}

pub fn sync_style_pack_preferences(prefs: &mut UserPreferences, packs: &[StylePack]) -> bool {
    let previous_active_style_pack_id = prefs.active_style_pack_id.clone();
    let previous_default_mode = prefs.default_mode;
    let previous_enabled_modes = prefs.enabled_modes.clone();
    let enabled: Vec<&StylePack> = packs.iter().filter(|pack| pack.enabled).collect();
    let active = packs
        .iter()
        .find(|pack| pack.id == prefs.active_style_pack_id && pack.enabled)
        .or_else(|| {
            packs
                .iter()
                .find(|pack| pack.id == builtin_style_pack_id(prefs.default_mode) && pack.enabled)
        })
        .or_else(|| enabled.first().copied());

    let Some(active_pack) = active else {
        return false;
    };

    let mut changed = false;
    if prefs.active_style_pack_id != active_pack.id {
        prefs.active_style_pack_id = active_pack.id.clone();
        changed = true;
    }
    if prefs.default_mode != active_pack.base_mode {
        prefs.default_mode = active_pack.base_mode;
        changed = true;
    }

    let next_enabled_modes = enabled_modes_from_style_packs(packs);
    if prefs.enabled_modes != next_enabled_modes {
        prefs.enabled_modes = next_enabled_modes;
        changed = true;
    }

    if sync_builtin_style_prompt_preferences(prefs, packs) {
        changed = true;
    }

    if changed {
        log::info!(
            "[style-pack] sync_prefs active:{}->{} default_mode:{:?}->{:?} enabled_modes:{:?}->{:?}",
            previous_active_style_pack_id,
            prefs.active_style_pack_id,
            previous_default_mode,
            prefs.default_mode,
            previous_enabled_modes,
            prefs.enabled_modes
        );
    }

    changed
}

fn sync_builtin_style_prompt_preferences(prefs: &mut UserPreferences, packs: &[StylePack]) -> bool {
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
        let next_prompt = pack.prompt.clone();
        let current_prompt = prefs.style_system_prompts.for_mode(mode);
        if current_prompt == next_prompt {
            continue;
        }
        match mode {
            PolishMode::Raw => prefs.style_system_prompts.raw = next_prompt,
            PolishMode::Light => prefs.style_system_prompts.light = next_prompt,
            PolishMode::Structured => prefs.style_system_prompts.structured = next_prompt,
            PolishMode::Formal => prefs.style_system_prompts.formal = next_prompt,
        }
        changed = true;
    }

    if saw_builtin && prefs.custom_style_prompts != CustomStylePrompts::default() {
        prefs.custom_style_prompts = CustomStylePrompts::default();
        changed = true;
    }

    changed
}

pub fn enabled_modes_from_style_packs(packs: &[StylePack]) -> Vec<PolishMode> {
    let mut modes = Vec::new();
    for mode in [
        PolishMode::Raw,
        PolishMode::Light,
        PolishMode::Structured,
        PolishMode::Formal,
    ] {
        if packs
            .iter()
            .any(|pack| pack.enabled && pack.base_mode == mode)
        {
            modes.push(mode);
        }
    }
    modes
}

fn builtin_mode_from_style_pack_id(id: &str) -> Option<PolishMode> {
    for mode in [
        PolishMode::Raw,
        PolishMode::Light,
        PolishMode::Structured,
        PolishMode::Formal,
    ] {
        if builtin_style_pack_id(mode) == id {
            return Some(mode);
        }
    }
    None
}

fn merge_style_pack_update(existing: StylePack, incoming: StylePack) -> Result<StylePack> {
    if existing.id != incoming.id {
        return Err(anyhow!("style pack id cannot be changed"));
    }
    let mut updated = existing;
    updated.name = normalize_required_text(&incoming.name, "style pack name")?;
    updated.description = incoming.description.trim().to_string();
    updated.author = normalize_optional_text(incoming.author);
    updated.version = normalize_version(&incoming.version);
    updated.prompt = incoming.prompt;
    updated.examples = normalize_examples(incoming.examples);
    updated.tags = normalize_tags(&incoming.tags);
    updated.recommended_model = normalize_optional_text(incoming.recommended_model);
    updated.compatible_app_version = normalize_optional_text(incoming.compatible_app_version);
    // origin 字段是 marketplace_install 之后的「衍生关系绑定」，**不能**走通用 save 路径覆盖
    // ——否则前端 save 时丢失 originPackId 就会清掉关联。要写 origin 走专用的 set_origin。
    updated.updated_at = Some(Utc::now().to_rfc3339());
    Ok(updated)
}

fn normalize_examples(examples: Vec<StylePackExample>) -> Vec<StylePackExample> {
    examples
        .into_iter()
        .filter_map(|example| {
            let input = example.input.trim().to_string();
            let output = example.output.trim().to_string();
            if input.is_empty() && output.is_empty() {
                return None;
            }
            Some(StylePackExample {
                title: normalize_optional_text(example.title),
                input,
                output,
            })
        })
        .collect()
}

fn normalize_tags(tags: &[String]) -> Vec<String> {
    let mut normalized = Vec::new();
    for tag in tags {
        let trimmed = tag.trim();
        if trimmed.is_empty() || normalized.iter().any(|existing| existing == trimmed) {
            continue;
        }
        normalized.push(trimmed.to_string());
    }
    normalized
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value.and_then(|text| {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn normalize_required_text(value: &str, field: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("{field} is empty"));
    }
    Ok(trimmed.to_string())
}

fn normalize_version(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "1.0.0".into()
    } else {
        trimmed.to_string()
    }
}

fn unique_imported_style_pack_id(existing: &[StylePack], requested_id: &str) -> String {
    let base = sanitize_style_pack_id(requested_id);
    if !existing.iter().any(|pack| pack.id == base) {
        return base;
    }
    let mut index = 2usize;
    loop {
        let candidate = format!("{base}-{index}");
        if !existing.iter().any(|pack| pack.id == candidate) {
            return candidate;
        }
        index = index.saturating_add(1);
    }
}

fn sanitize_style_pack_id(requested_id: &str) -> String {
    let mut output = String::new();
    for ch in requested_id.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch.to_ascii_lowercase());
        } else if matches!(ch, '-' | '_' | '.') {
            output.push(ch);
        } else if matches!(ch, ' ' | '/' | '\\') {
            output.push('-');
        }
    }
    let compact = output.trim_matches('-').trim_matches('.').trim_matches('_');
    if compact.is_empty() {
        format!("imported-{}", Uuid::new_v4().simple())
    } else if compact.starts_with("builtin.") {
        format!("imported.{compact}")
    } else {
        compact.to_string()
    }
}

fn read_zip_json_entry<T: for<'de> Deserialize<'de>>(
    archive: &mut zip::ZipArchive<fs::File>,
    entry_name: &str,
) -> Result<T> {
    let text = read_zip_string_entry(archive, entry_name)?;
    serde_json::from_str(&text)
        .with_context(|| format!("decode style pack zip entry failed: {entry_name}"))
}

fn read_zip_string_entry(
    archive: &mut zip::ZipArchive<fs::File>,
    entry_name: &str,
) -> Result<String> {
    let mut file = archive
        .by_name(entry_name)
        .with_context(|| format!("missing style pack zip entry: {entry_name}"))?;
    let mut buffer = String::new();
    file.read_to_string(&mut buffer)
        .with_context(|| format!("read style pack zip entry failed: {entry_name}"))?;
    Ok(buffer)
}

fn extract_style_pack_icon(
    archive: &mut zip::ZipArchive<fs::File>,
    asset_root: &Path,
    pack_id: &str,
    entry_name: &str,
) -> Result<Option<String>> {
    let mut file = archive
        .by_name(entry_name)
        .with_context(|| format!("missing style pack icon entry: {entry_name}"))?;
    let file_name = Path::new(entry_name)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("invalid style pack icon file name"))?;
    let target_dir = asset_root.join(pack_id);
    ensure_dir(&target_dir)?;
    let target_path = target_dir.join(file_name);
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .with_context(|| format!("read style pack icon failed: {entry_name}"))?;
    fs::write(&target_path, &bytes)
        .with_context(|| format!("write style pack icon failed: {}", target_path.display()))?;
    Ok(Some(target_path.to_string_lossy().to_string()))
}

fn remove_style_pack_assets(asset_root: &Path, pack: &StylePack) {
    if let Some(icon_path) = pack.icon_path.as_deref() {
        let path = Path::new(icon_path);
        let _ = fs::remove_file(path);
        if let Some(parent) = path.parent() {
            let _ = fs::remove_dir(parent);
        }
    } else {
        let dir = asset_root.join(&pack.id);
        let _ = fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod tests {
    use super::sync_style_pack_preferences;
    use crate::types::{builtin_style_packs, CustomStylePrompts};

    #[test]
    fn sync_style_pack_preferences_uses_builtin_store_prompts_as_source_of_truth() {
        let mut prefs = crate::types::UserPreferences {
            style_system_prompts: crate::types::StyleSystemPrompts {
                raw: "stale raw".into(),
                light: "stale light".into(),
                structured: "stale structured".into(),
                formal: "stale formal".into(),
            },
            custom_style_prompts: CustomStylePrompts {
                raw: String::new(),
                light: "legacy extra instruction".into(),
                structured: String::new(),
                formal: String::new(),
            },
            ..Default::default()
        };
        let mut packs = builtin_style_packs();
        let light = packs
            .iter_mut()
            .find(|pack| pack.id == "builtin.light")
            .expect("builtin light pack");
        light.prompt = "fresh light prompt from store".into();

        assert!(sync_style_pack_preferences(&mut prefs, &packs));
        assert_eq!(prefs.style_system_prompts.raw, packs[0].prompt);
        assert_eq!(
            prefs.style_system_prompts.light,
            "fresh light prompt from store"
        );
        assert_eq!(prefs.style_system_prompts.structured, packs[2].prompt);
        assert_eq!(prefs.style_system_prompts.formal, packs[3].prompt);
        assert_eq!(prefs.custom_style_prompts, CustomStylePrompts::default());
    }
}
