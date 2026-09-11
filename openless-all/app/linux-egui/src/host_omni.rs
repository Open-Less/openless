#[derive(Clone, Default)]
struct OmniEditor {
    provider: String,
    endpoint: String,
    model: String,
    secret: String,
    headers: String,
    temperature: String,
    models: Vec<String>,
}
impl OpenLessEguiApp {
    fn load_omni(&mut self, requested: Option<String>) {
        if self.omni_loading {
            return;
        }
        let Some(backend) = self.backend() else {
            return;
        };
        self.omni_loading = true;
        self.omni_error=None;
        self.omni = None;
        let tx = self.tx.clone();
        self.tokio.spawn(async move {
            let result: Result<OmniEditor, BackendError> = async {
                let provider =
                    requested.unwrap_or_else(|| backend.get_preferences().active_omni_provider);
                let descriptor = openless_core::provider_rules::provider_descriptor(
                    openless_core::ProviderKind::Omni,
                    &provider,
                )
                .ok_or_else(|| {
                    BackendError::new(
                        openless_core::BackendErrorCode::InvalidArgument,
                        "未知的 Omni 服务",
                    )
                })?;
                let mut editor = OmniEditor {
                    provider: provider.clone(),
                    endpoint: descriptor.default_endpoint.unwrap_or_default(),
                    model: descriptor.default_model.unwrap_or_default(),
                    models: descriptor.static_models,
                    ..Default::default()
                };
                for (account, field) in [
                    (
                        openless_core::credentials::OMNI_ENDPOINT_ACCOUNT,
                        &mut editor.endpoint,
                    ),
                    (
                        openless_core::credentials::OMNI_MODEL_ACCOUNT,
                        &mut editor.model,
                    ),
                    (
                        openless_core::credentials::OMNI_EXTRA_HEADERS_ACCOUNT,
                        &mut editor.headers,
                    ),
                    (
                        openless_core::credentials::OMNI_TEMPERATURE_ACCOUNT,
                        &mut editor.temperature,
                    ),
                ] {
                    let key = openless_core::CredentialKey::new(
                        openless_core::CredentialNamespace::Omni,
                        Some(provider.clone()),
                        account,
                    )?;
                    if let Some(value) = backend.read_credential(key).await? {
                        *field = value.into_exposed();
                    }
                }
                Ok(editor)
            }
            .await;
            let _ = tx.send(UiResult::Omni(result.map_err(|e| e.to_string())));
        });
    }
    fn omni_v2(&mut self, ui: &mut egui::Ui) {
        if let Some(error)=self.omni_error.clone() {
            ui.label(error);if ui.button(theme::text("重试")).clicked(){self.load_omni(None);}return;
        }
        if self.omni.is_none() && !self.omni_loading {
            self.load_omni(None);
        }
        if self.omni_loading {
            ui.spinner();
            return;
        }
        let Some(mut editor) = self.omni.clone() else {
            return;
        };
        crate::ui::settings::card(ui, "Omni 多模态", |ui| {
            let mut provider = editor.provider.clone();
            egui::ComboBox::from_id_salt("omni-provider")
                .selected_text(&provider)
                .show_ui(ui, |ui| {
                    for descriptor in openless_core::provider_rules::provider_descriptors(
                        openless_core::ProviderKind::Omni,
                    ) {
                        ui.selectable_value(
                            &mut provider,
                            descriptor.provider_type.to_string(),
                            theme::text_key(&descriptor.label_key),
                        );
                    }
                });
            if provider != editor.provider {
                self.load_omni(Some(provider));
                return;
            }
            for (label, value) in [
                ("API 地址", &mut editor.endpoint),
                ("模型", &mut editor.model),
                ("Temperature", &mut editor.temperature),
            ] {
                ui.horizontal(|ui| {
                    ui.label(theme::text(label));
                    ui.text_edit_singleline(value);
                });
            }
            ui.horizontal(|ui| {
                ui.label("API Key");
                ui.add(
                    egui::TextEdit::singleline(&mut editor.secret)
                        .password(true)
                        .hint_text(theme::text("留空保留已保存的密钥")),
                );
            });
            ui.label(theme::text("附加请求头（JSON）"));
            ui.add(
                egui::TextEdit::multiline(&mut editor.headers)
                    .desired_rows(2)
                    .desired_width(f32::INFINITY),
            );
            egui::ComboBox::from_id_salt("omni-models")
                .selected_text(theme::text("选择模型"))
                .show_ui(ui, |ui| {
                    for model in &editor.models {
                        ui.selectable_value(&mut editor.model, model.clone(), model);
                    }
                });
            let mut operation = None;
            ui.horizontal_wrapped(|ui| {
                for (key, label) in [
                    ("save", "保存并启用"),
                    ("test", "测试连接"),
                    ("models", "获取模型"),
                    ("clear", "清除密钥"),
                ] {
                    if ui.button(theme::text(label)).clicked() {
                        operation = Some(key);
                    }
                }
            });
            if let Some(operation) = operation {
                if let Some(backend) = self.backend() {
                    let saved = editor.clone();
                    editor.secret.clear();
                    let tx = self.tx.clone();
                    self.tokio.spawn(async move {
                        let result: Result<String, BackendError> = async {
                            use openless_core::credentials::*;
                            let key = |account| {
                                openless_core::CredentialKey::new(
                                    openless_core::CredentialNamespace::Omni,
                                    Some(saved.provider.clone()),
                                    account,
                                )
                            };
                            if operation == "clear" {
                                backend
                                    .remove_credential(key(OMNI_API_KEY_ACCOUNT)?)
                                    .await?;
                                return Ok("密钥已清除".into());
                            }
                            if !saved.headers.trim().is_empty() {
                                let parsed: serde_json::Value =
                                    serde_json::from_str(&saved.headers).map_err(|e| {
                                        BackendError::new(
                                            openless_core::BackendErrorCode::InvalidArgument,
                                            e.to_string(),
                                        )
                                    })?;
                                if !parsed
                                    .as_object()
                                    .is_some_and(|o| o.values().all(|v| v.is_string()))
                                {
                                    return Err(BackendError::new(
                                        openless_core::BackendErrorCode::InvalidArgument,
                                        "请求头必须是 JSON 字符串映射",
                                    ));
                                }
                            }
                            if !saved.temperature.trim().is_empty()
                                && !saved
                                    .temperature
                                    .parse::<f64>()
                                    .ok()
                                    .is_some_and(|n| n.is_finite() && (0.0..=2.0).contains(&n))
                            {
                                return Err(BackendError::new(
                                    openless_core::BackendErrorCode::InvalidArgument,
                                    "Temperature 应在 0 到 2 之间",
                                ));
                            }
                            for (account, value) in [
                                (OMNI_ENDPOINT_ACCOUNT, saved.endpoint),
                                (OMNI_MODEL_ACCOUNT, saved.model),
                                (OMNI_EXTRA_HEADERS_ACCOUNT, saved.headers),
                                (OMNI_TEMPERATURE_ACCOUNT, saved.temperature),
                            ] {
                                if value.trim().is_empty() {
                                    backend.remove_credential(key(account)?).await?;
                                } else {
                                    backend
                                        .set_credential(
                                            key(account)?,
                                            openless_core::SecretValue::new(value),
                                        )
                                        .await?;
                                }
                            }
                            if !saved.secret.is_empty() {
                                backend
                                    .set_credential(
                                        key(OMNI_API_KEY_ACCOUNT)?,
                                        openless_core::SecretValue::new(saved.secret),
                                    )
                                    .await?;
                            }
                            backend
                                .set_active_provider(
                                    openless_core::ProviderSlot::Omni,
                                    saved.provider.clone(),
                                )
                                .await?;
                            let request = openless_core::ProviderRequest {
                                kind: openless_core::ProviderKind::Omni,
                                channel_id: Some(saved.provider.clone()),
                                thinking_enabled: false,
                            };
                            match operation {
                                "test" => {
                                    let status =
                                        backend.services().provider.validate(request).await?;
                                    Ok(if status.ok {
                                        "连接成功"
                                    } else {
                                        "连接失败"
                                    }
                                    .into())
                                }
                                "models" => {
                                    let models = backend
                                        .services()
                                        .provider
                                        .list_models(request)
                                        .await?
                                        .models;
                                    let _ = tx.send(UiResult::OmniModels(saved.provider,Ok(models)));
                                    Ok("模型列表已更新".into())
                                }
                                _ => Ok("Omni 配置已保存".into()),
                            }
                        }
                        .await;
                        let _ =
                            tx.send(UiResult::Message(result.unwrap_or_else(|e| e.to_string())));
                    });
                }
            }
        });
        if !self.omni_loading {
            self.omni = Some(editor);
        }
    }
}
