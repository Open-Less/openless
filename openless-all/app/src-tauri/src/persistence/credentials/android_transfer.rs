//! Explicitly user-selected, plaintext Android device transfer. No OAuth/sync secrets.
use super::*;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Channel {
    id: String,
    provider_type: String,
    name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    configuration: Option<serde_json::Value>,
}
#[derive(Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Channels {
    asr: Option<Channel>,
    llm: Option<Channel>,
}

fn export_channel<T: Serialize + HasChannelMeta>(
    id: &str,
    entry: &T,
    secrets: bool,
) -> Result<Channel> {
    let mut value = serde_json::to_value(entry)?;
    let object = value.as_object_mut().context("invalid channel")?;
    let name = object
        .get("displayName")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    for key in [
        "providerType",
        "displayName",
        "order",
        "enabled",
        "lastTest",
    ] {
        object.remove(key);
    }
    Ok(Channel {
        id: id.into(),
        provider_type: channel_provider_type(id, entry).into(),
        name,
        configuration: secrets.then_some(value),
    })
}

fn merge_channel<T: Serialize + serde::de::DeserializeOwned + Default + HasChannelMeta>(
    map: &mut HashMap<String, T>,
    channel: &Channel,
    secrets: bool,
) -> Result<()> {
    anyhow::ensure!(
        !channel.id.trim().is_empty()
            && channel.id.len() <= 256
            && !channel.provider_type.trim().is_empty(),
        "invalid channel identity"
    );
    if let Some(existing) = map.get(&channel.id) {
        anyhow::ensure!(
            channel_provider_type(&channel.id, existing) == channel.provider_type,
            "channel identity conflicts with the target device"
        );
    }
    let mut value = map
        .get(&channel.id)
        .map(serde_json::to_value)
        .transpose()?
        .unwrap_or_else(|| serde_json::json!({}));
    if secrets {
        let imported = channel
            .configuration
            .as_ref()
            .context("credential configuration is missing")?;
        anyhow::ensure!(imported.is_object(), "invalid credential configuration");
        // Decode to the actual entry type first; metadata comes only from the explicit identity.
        let _: T = serde_json::from_value(imported.clone()).context("invalid credential fields")?;
        for (key, field) in imported.as_object().unwrap() {
            anyhow::ensure!(
                ![
                    "providerType",
                    "displayName",
                    "order",
                    "enabled",
                    "lastTest"
                ]
                .contains(&key.as_str()),
                "metadata inside credentials is not allowed"
            );
            value
                .as_object_mut()
                .unwrap()
                .insert(key.clone(), field.clone());
        }
    }
    let object = value.as_object_mut().context("invalid channel")?;
    object.insert("displayName".into(), serde_json::to_value(&channel.name)?);
    object.insert("providerType".into(), channel.provider_type.clone().into());
    object.insert(
        "order".into(),
        map.get(&channel.id)
            .and_then(|v| v.meta().order)
            .unwrap_or(map.len() as u32)
            .into(),
    );
    object.insert(
        "enabled".into(),
        map.get(&channel.id)
            .map(|v| v.meta().enabled)
            .unwrap_or(true)
            .into(),
    );
    object.remove("lastTest");
    map.insert(channel.id.clone(), serde_json::from_value(value)?);
    Ok(())
}

fn merge_channels(
    root: &CredsRoot,
    channels: &Channels,
    activate: bool,
    secrets: bool,
) -> Result<CredsRoot> {
    for (channel, kind) in [
        (
            channels.asr.as_ref(),
            openless_core::domains::ProviderKind::Asr,
        ),
        (
            channels.llm.as_ref(),
            openless_core::domains::ProviderKind::Llm,
        ),
    ] {
        if let Some(channel) = channel {
            anyhow::ensure!(
                openless_core::provider_rules::provider_descriptor(kind, &channel.provider_type)
                    .is_some(),
                "unsupported provider type"
            );
        }
    }
    let mut next = root.clone();
    if let Some(channel) = &channels.asr {
        merge_channel(&mut next.providers.asr, channel, secrets)?;
    }
    if let Some(channel) = &channels.llm {
        merge_channel(&mut next.providers.llm, channel, secrets)?;
    }
    if activate {
        for (slot, channel) in [
            (openless_core::ProviderSlot::Asr, &channels.asr),
            (openless_core::ProviderSlot::Llm, &channels.llm),
        ] {
            if let Some(channel) = channel {
                let mut metadata = credential_metadata(&next);
                metadata
                    .select_active_provider(slot, channel.id.clone())
                    .map_err(anyhow::Error::new)?;
                if metadata.revision() != next.metadata_revision {
                    apply_credential_metadata(&mut next, metadata)?;
                }
            }
        }
    }
    // Reuse the existing provider/account validation without exposing sync secrets.
    export_sync_credentials_root(&next)?;
    Ok(next)
}

impl CredentialsVault {
    pub(crate) fn export_android_channels(secrets: bool) -> Result<serde_json::Value> {
        let _guard = credentials_lock().lock();
        let root = load_credentials_for_update()?;
        let channels = Channels {
            asr: root
                .providers
                .asr
                .get(&root.active.asr)
                .map(|v| export_channel(&root.active.asr, v, secrets))
                .transpose()?,
            llm: root
                .providers
                .llm
                .get(&root.active.llm)
                .map(|v| export_channel(&root.active.llm, v, secrets))
                .transpose()?,
        };
        serde_json::to_value(channels).map_err(Into::into)
    }
    pub(crate) fn import_android_channels(
        value: serde_json::Value,
        activate: bool,
        secrets: bool,
    ) -> Result<()> {
        let channels: Channels =
            serde_json::from_value(value).context("invalid channel transfer")?;
        mutate_credentials(openless_core::credentials::ChangeOrigin::User, |root| {
            *root = merge_channels(root, &channels, activate, secrets)?;
            Ok(true)
        })
    }
}

impl CredentialsVault {
    pub(crate) fn legacy_android_channels(
        selection: &serde_json::Value,
        credentials: Option<&serde_json::Value>,
    ) -> Result<serde_json::Value> {
        let _guard = credentials_lock().lock();
        let root = load_credentials_for_update()?;
        let mut channels = Channels::default();
        for (slot, field) in [("asr", "activeAsrProvider"), ("llm", "activeLlmProvider")] {
            let Some(id) = selection.get(field).and_then(|v| v.as_str()) else {
                anyhow::ensure!(credentials.is_none(), "legacy credentials have no channel identity; export again on the source device");
                continue;
            };
            let mut channel = if slot == "asr" {
                export_channel(
                    id,
                    root.providers
                        .asr
                        .get(id)
                        .context("legacy ASR channel is unknown; export again")?,
                    false,
                )?
            } else {
                export_channel(
                    id,
                    root.providers
                        .llm
                        .get(id)
                        .context("legacy LLM channel is unknown; export again")?,
                    false,
                )?
            };
            if let Some(credentials) = credentials {
                let mut configuration = serde_json::Map::new();
                let fields: &[(&str, &str)] = if slot == "asr" {
                    &[
                        ("volcengineAppKey", "appKey"),
                        ("volcengineAccessKey", "accessKey"),
                        ("volcengineResourceId", "resourceId"),
                        ("volcengineService", "volcengineService"),
                        ("volcengineAuthMode", "authMode"),
                        ("volcengineApiKey", "volcengineApiKey"),
                        ("asrApiKey", "apiKey"),
                        ("asrEndpoint", "baseURL"),
                        ("asrModel", "model"),
                        ("asrVocabularyId", "vocabularyId"),
                        ("asrAdvancedConfig", "advancedConfig"),
                        ("xfyunAppId", "xfyunAppId"),
                        ("xfyunApiKey", "xfyunApiKey"),
                        ("tencentCloudAppId", "tencentCloudAppId"),
                        ("tencentCloudSecretId", "tencentCloudSecretId"),
                        ("tencentCloudSecretKey", "tencentCloudSecretKey"),
                    ]
                } else {
                    &[
                        ("arkApiKey", "apiKey"),
                        ("arkEndpoint", "baseURL"),
                        ("arkModelId", "model"),
                    ]
                };
                for (from, to) in fields {
                    if let Some(value) = credentials.get(from).filter(|v| !v.is_null()) {
                        configuration.insert((*to).into(), value.clone());
                    }
                }
                channel.configuration = Some(configuration.into());
            }
            if slot == "asr" {
                channels.asr = Some(channel);
            } else {
                channels.llm = Some(channel);
            }
        }
        Ok(serde_json::to_value(channels)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activating_imported_slots_is_repeatable_for_every_selection() {
        for (asr, llm) in [(false, false), (true, false), (false, true), (true, true)] {
            let channels = Channels {
                asr: asr.then(|| Channel {
                    id: "imported-asr".into(),
                    provider_type: "openai-compatible".into(),
                    name: Some("Imported ASR".into()),
                    configuration: None,
                }),
                llm: llm.then(|| Channel {
                    id: "imported-llm".into(),
                    provider_type: "deepseek".into(),
                    name: Some("Imported LLM".into()),
                    configuration: None,
                }),
            };
            let root = CredsRoot::default();
            let restored = merge_channels(&root, &channels, true, false).unwrap();
            if asr {
                assert_eq!(restored.active.asr, "imported-asr");
            }
            if llm {
                assert_eq!(restored.active.llm, "imported-llm");
            }
            let repeated = merge_channels(&restored, &channels, true, false).unwrap();
            assert_eq!(
                serde_json::to_value(&restored).unwrap(),
                serde_json::to_value(&repeated).unwrap(),
                "asr={asr}, llm={llm}"
            );
        }
    }

    #[test]
    fn transfer_is_bound_to_identity_and_preserves_other_channels() {
        let mut root = CredsRoot::default();
        root.providers.llm.insert(
            "old".into(),
            CredsLlmEntry {
                channel: ChannelMeta {
                    providerType: Some("deepseek".into()),
                    ..Default::default()
                },
                apiKey: Some("keep".into()),
                ..Default::default()
            },
        );
        root.active.llm = "old".into();
        let channels = Channels {
            asr: None,
            llm: Some(Channel {
                id: "imported".into(),
                provider_type: "openai-compatible".into(),
                name: Some("Restored".into()),
                configuration: Some(
                    serde_json::json!({"apiKey":"new", "baseURL":"https://example.com/v1", "model":"test"}),
                ),
            }),
        };
        let restored = merge_channels(&root, &channels, true, true).unwrap();
        assert_eq!(
            restored.providers.llm["old"].apiKey.as_deref(),
            Some("keep")
        );
        assert_eq!(
            restored.providers.llm["imported"].apiKey.as_deref(),
            Some("new")
        );
        assert_eq!(restored.active.llm, "imported");
        assert_eq!(
            merge_channels(&restored, &channels, true, true)
                .unwrap()
                .providers
                .llm
                .len(),
            2
        );
        let mut conflict = channels;
        conflict.llm.as_mut().unwrap().id = "old".into();
        assert!(merge_channels(&root, &conflict, true, true).is_err());
        assert_eq!(root.providers.llm["old"].apiKey.as_deref(), Some("keep"));
        let exported = export_channel("old", &root.providers.llm["old"], false).unwrap();
        assert!(exported.configuration.is_none());
        assert_eq!(
            merge_channels(
                &root,
                &Channels {
                    asr: None,
                    llm: Some(exported)
                },
                false,
                false
            )
            .unwrap()
            .providers
            .llm["old"]
                .apiKey
                .as_deref(),
            Some("keep")
        );
    }
}
