impl OpenLessEguiApp {
    fn load_local_models(&mut self) {
        if self.local_models_loading {
            return;
        }
        let Some(backend) = self.backend() else {
            return;
        };
        self.local_models_loading = true;
        let tx = self.tx.clone();
        self.tokio.spawn(async move {
            let result = backend
                .services()
                .local_asr
                .list_models(openless_core::LocalAsrRuntime::Generic)
                .await
                .map_err(|e| e.to_string());
            let _ = tx.send(UiResult::LocalModels(result));
        });
    }

    fn local_models_v2(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut crate::ui::settings::SettingsState,
    ) {
        use crate::ui::settings::card;
        if self.local_models.is_none() {
            self.load_local_models();
        }
        card(ui, "本地语音识别", |ui| {
            ui.label(theme::text("在本机处理录音。下载完成后可启用、测试或释放模型。"));
            if ui
                .add_enabled(
                    !self.local_models_loading,
                    egui::Button::new("刷新模型列表"),
                )
                .clicked()
            {
                self.load_local_models();
            }
            if self.local_models_loading {
                ui.spinner();
            }
        });
        for model in self.local_models.clone().unwrap_or_default() {
            let target = model.target.clone();
            let id = target.model_id().to_owned();
            card(ui, &model.display_name, |ui| {
                ui.label(format!(
                    "{} · {}",
                    model.family,
                    if model.installed {
                        "已下载"
                    } else {
                        "尚未下载"
                    }
                ));
                if let Some(size) = model.size_bytes {
                    ui.label(format!("{:.1} MB", size as f64 / 1_048_576.0));
                }
                if let Some(progress)=self.model_downloads.get(&id) {
                    ui.label(format!("{} · {}/{}",progress.file,progress.file_index,progress.file_count));
                    if progress.bytes_total>0 {ui.add(egui::ProgressBar::new(progress.bytes_downloaded as f32 / progress.bytes_total as f32).show_percentage());}
                    if let Some(error)=&progress.error {ui.colored_label(egui::Color32::from_rgb(220, 38, 38),error);}
                }
                if let Some(progress)=self.model_prepare.as_ref().filter(|p|p.model_alias==id) {
                    ui.label(&progress.label); if let Some(percent)=progress.percent {ui.add(egui::ProgressBar::new((percent/100.0) as f32).show_percentage());}
                    if let Some(error)=&progress.error {ui.colored_label(egui::Color32::from_rgb(220, 38, 38),error);}
                }
                let mut action = None;
                ui.horizontal_wrapped(|ui| {
                    if model.installed {
                        for (key, label) in [
                            ("activate", "启用"),
                            ("test", "测试模型"),
                            ("release", "释放内存"),
                            ("folder", "打开目录"),
                            ("delete", "删除模型"),
                        ] {
                            if ui.button(label).clicked() {
                                action = Some(key);
                            }
                        }
                    } else {
                        for (key, label) in [
                            ("download", "下载"),
                            ("cancel", "取消下载"),
                            ("cleanup", "清理未完成下载"),
                        ] {
                            if ui.button(label).clicked() {
                                action = Some(key);
                            }
                        }
                    }
                    if ui.button(theme::text("取消加载")).clicked() {
                        action = Some("cancel_prepare");
                    }
                });
                if action == Some("delete") {
                    state.confirmation = Some(format!("model:{id}"));
                    action = None;
                }
                if state.confirmation.as_deref() == Some(format!("model:{id}").as_str()) {
                    ui.label(theme::text("确认删除此模型文件？"));
                    ui.horizontal(|ui| {
                        if ui.button(theme::text("确认删除")).clicked() {
                            action = Some("delete");
                            state.confirmation = None;
                        }
                        if ui.button(theme::text("取消")).clicked() {
                            state.confirmation = None;
                        }
                    });
                }
                if let Some(action) = action {
                    self.local_model_action(target, action);
                }
            });
        }
        card(ui, "存储与下载", |ui| {
            let document = self
                .preferences
                .as_ref()
                .and_then(|p| serde_json::to_value(p).ok())
                .unwrap_or_default();
            let mut edits = std::collections::BTreeMap::new();
            preference_field(
                ui,
                &document,
                &mut edits,
                "/localAsrMirror",
                "下载源",
                FieldKind::Choice(&[
                    ("huggingface", "Hugging Face"),
                    ("hf-mirror", "HF Mirror"),
                ]),
            );
            preference_field(
                ui,
                &document,
                &mut edits,
                "/localAsrKeepLoadedSecs",
                "模型保留时间（秒）",
                FieldKind::Number(0.0, 86400.0),
            );
            self.save_field_edits(edits);
            if ui.button(theme::text("更改模型目录…")).clicked() {
                if let Some(backend) = self.backend() {
                    self.spawn(async move {
                        let folder = rfd::AsyncFileDialog::new().pick_folder().await;
                        if let Some(folder) = folder {
                            backend
                                .services()
                                .local_asr
                                .set_models_base_dir(Some(folder.path().to_owned()))
                                .await?;
                            Ok("模型目录已保存".into())
                        } else {
                            Ok("已取消".into())
                        }
                    });
                }
            }
        });
    }

    fn local_model_action(&self, target: openless_core::LocalAsrTarget, action: &str) {
        let Some(backend) = self.backend() else {
            return;
        };
        let action = action.to_owned();
        let tx = self.tx.clone();
        self.tokio.spawn(async move {
            let api = backend.services().local_asr.clone();
            let result: Result<String, BackendError> = async {
                match action.as_str() {
                    "download" => api.start_download(target, None).await?,
                    "cancel" => api.cancel_download(target).await?,
                    "cleanup" => api.cleanup_incomplete(target).await?,
                    "cancel_prepare" => api.cancel_prepare(target.runtime).await?,
                    "activate" => {
                        api.activate(openless_core::LocalAsrActivationRequest {
                            provider_id: if target.model_id().starts_with("whisper") {
                                "local-whisper".into()
                            } else {
                                target.runtime.provider_id().into()
                            },
                            target,
                        })
                        .await?;
                    }
                    "release" => api.release(target.runtime).await?,
                    "delete" => api.delete_model(target).await?,
                    "test" => {
                        let result = api.test_model(target).await?;
                        return Ok(format!(
                            "{} · {} ms",
                            result.transcribed_text, result.transcribe_ms
                        ));
                    }
                    "folder" => {
                        let path = api.model_dir(target).await?;
                        tokio::task::spawn_blocking(move || {
                            openless_linux_egui::open_local_file(&path)
                        })
                        .await
                        .map_err(|e| {
                            BackendError::new(
                                openless_core::BackendErrorCode::Platform,
                                e.to_string(),
                            )
                        })?
                        .map_err(|e| {
                            BackendError::new(
                                openless_core::BackendErrorCode::Platform,
                                e.to_string(),
                            )
                        })?;
                    }
                    _ => unreachable!(),
                }
                Ok("模型操作完成".into())
            }
            .await;
            let _ = tx.send(UiResult::LocalModelAction(
                result.map_err(|e| e.to_string()),
            ));
        });
    }

    fn marketplace_account_v2(&mut self, ui: &mut egui::Ui) {
        if ui.button(theme::text("使用 GitHub 登录")).clicked() {
            if let Some(backend) = self.backend() {
                let tx = self.tx.clone();
                self.tokio.spawn(async move {
                    let result = backend
                        .services()
                        .marketplace
                        .start_device_flow()
                        .await
                        .map_err(|e| e.to_string());
                    let _ = tx.send(UiResult::MarketplaceFlow(result));
                });
            }
        }
        if let Some(flow) = self.marketplace_flow.clone() {
            ui.monospace(&flow.user_code);
            ui.hyperlink_to("打开 GitHub 授权页", &flow.verification_uri);
            if ui.button(theme::text("检查授权状态")).clicked() {
                if let Some(backend) = self.backend() {
                    let tx = self.tx.clone();
                    let flow_id=flow.flow_id.clone();
                    self.tokio.spawn(async move {
                        let result = backend
                            .services()
                            .marketplace
                            .poll_device_flow(flow_id)
                            .await
                            .map_err(|e| e.to_string());
                        let _ = tx.send(UiResult::MarketplaceAuthPoll(result));
                    });
                }
            }
            if ui.button(theme::text("取消登录")).clicked() {
                if let Some(backend)=self.backend() {
                    let id=flow.flow_id.clone();
                    self.spawn(async move {backend.services().marketplace.cancel_device_flow(Some(id)).await?;Ok("已取消登录".into())});
                }
                self.marketplace_flow = None;
            }
        }
        if ui.button(theme::text("退出登录")).clicked() {
            if let Some(backend) = self.backend() {
                self.spawn(async move {
                    backend.services().marketplace.logout().await?;
                    Ok("已退出登录".into())
                });
            }
        }
    }
}
