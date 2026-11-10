//! Android in-app updater: manifest fetch, minisign verify, system APK install.

#[cfg(target_os = "android")]
mod android_impl {
    use std::path::PathBuf;

    use tauri::{AppHandle, Emitter};

    use crate::android::updater_logic::{map_abi_to_arch, INSTALLER_NOT_OPENED_MSG};
    use crate::commands::AppUpdateMetadata;
    use crate::types::UpdateChannel;

    #[derive(Clone, serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct AndroidUpdateProgress {
        pub downloaded: u64,
        pub content_length: Option<u64>,
        pub phase: String,
    }

    fn device_arch() -> Result<&'static str, String> {
        crate::android::jni::android::with_android_env(|env, _context| {
            // SUPPORTED_ABIS is a static field, not a method — call_static_method crashes.
            let abis_obj = env
                .get_static_field("android/os/Build", "SUPPORTED_ABIS", "[Ljava/lang/String;")
                .and_then(|value| value.l())
                .map_err(|e| format!("read SUPPORTED_ABIS: {e}"))?;
            let abis_array = jni::objects::JObjectArray::from(abis_obj);
            let len = env
                .get_array_length(&abis_array)
                .map_err(|e| format!("SUPPORTED_ABIS length: {e}"))?;
            if len == 0 {
                return Err("SUPPORTED_ABIS is empty".to_string());
            }
            let first = env
                .get_object_array_element(&abis_array, 0)
                .map_err(|e| format!("SUPPORTED_ABIS[0]: {e}"))?;
            let abi = env
                .get_string(&jni::objects::JString::from(first))
                .map_err(|e| format!("read abi string: {e}"))?
                .to_string_lossy()
                .into_owned();
            Ok(map_abi_to_arch(&abi))
        })
    }

    pub async fn check_update(channel: UpdateChannel) -> Result<Option<AppUpdateMetadata>, String> {
        let arch = device_arch()?;
        let current = env!("CARGO_PKG_VERSION").to_string();
        let Some(candidate) = super::super::update_pipeline::check(channel, arch, &current).await?
        else {
            return Ok(None);
        };
        let raw_json = serde_json::json!({
            "version": candidate.version, "url": candidate.url, "signature": candidate.signature,
            "pubDate": candidate.pub_date, "notes": candidate.notes,
        });
        Ok(Some(AppUpdateMetadata {
            rid: 0,
            current_version: current,
            version: candidate.version,
            date: candidate.pub_date,
            body: candidate.notes,
            raw_json,
        }))
    }

    fn updates_cache_dir() -> Result<PathBuf, String> {
        crate::android::jni::android::app_cache_dir()
            .map(|path| PathBuf::from(path).join("updates"))
    }

    pub async fn download_and_install(
        app: AppHandle,
        url: String,
        signature: String,
        version: String,
    ) -> Result<(), String> {
        semver::Version::parse(&version).map_err(|_| "invalid update version")?;
        let arch = device_arch()?;
        let bytes = super::super::update_pipeline::download(
            version.clone(),
            arch,
            url,
            signature,
            |downloaded, content_length| {
                let _ = app.emit(
                    "android-update:progress",
                    AndroidUpdateProgress {
                        downloaded,
                        content_length,
                        phase: "downloading".into(),
                    },
                );
            },
        )
        .await?;
        let cache_dir = updates_cache_dir()?;
        std::fs::create_dir_all(&cache_dir).map_err(|e| format!("create cache dir: {e}"))?;
        let dest = cache_dir.join(format!("OpenLess_{version}.apk"));
        std::fs::write(&dest, &bytes).map_err(|e| format!("write apk: {e}"))?;
        let downloaded = bytes.len() as u64;
        let total = Some(downloaded);

        let _ = app.emit(
            "android-update:progress",
            AndroidUpdateProgress {
                downloaded,
                content_length: total,
                phase: "installing".to_string(),
            },
        );

        let path = dest
            .to_str()
            .ok_or_else(|| "apk path is not UTF-8".to_string())?
            .to_string();
        let opened = crate::android::jni::android::with_android_env(|env, context| {
            let path_obj = crate::android::jni::android::jobject_str(env, &path)?;
            crate::android::jni::android::install_apk_from_path(env, context, &path_obj)
        })?;
        if !opened {
            return Err(INSTALLER_NOT_OPENED_MSG.to_string());
        }
        Ok(())
    }
}

#[cfg(target_os = "android")]
pub use android_impl::*;
