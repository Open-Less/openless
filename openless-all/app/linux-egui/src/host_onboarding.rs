impl OpenLessEguiApp {
    fn desktop_component(&self, mode: &'static str) {
        self.spawn(async move {
            tokio::task::spawn_blocking(move || {
                openless_linux_egui::desktop_bridge::manage_component(mode)
            })
            .await
            .map_err(|e| {
                BackendError::new(openless_core::BackendErrorCode::Platform, e.to_string())
            })?
        });
    }
    fn onboarding_v2(&mut self, ctx: &egui::Context) {
        let id = egui::Id::new("onboarding-completed");
        let completed = ctx.data(|d| d.get_temp::<bool>(id)).unwrap_or_else(|| {
            let completed = openless_linux_egui::load_ui_value("onboardingComplete")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            ctx.data_mut(|d| d.insert_temp(id, completed));
            completed
        });
        if completed {
            return;
        }
        egui::Modal::new(egui::Id::new("onboarding-2.0"))
            .frame(
                egui::Frame::new()
                    .fill(theme::surface())
                    .stroke(egui::Stroke::new(0.5, theme::line()))
                    .corner_radius(14)
                    .inner_margin(32),
            )
            .show(ctx, |ui| {
                ui.set_width((ctx.content_rect().width() - 104.0).clamp(240.0, 456.0));
                ui.label(
                    egui::RichText::new("OpenLess")
                        .size(24.0)
                        .strong()
                        .color(theme::blue()),
                );
                ui.add_space(16.0);
                ui.heading(theme::text_key("onboarding.welcome"));
                ui.label(theme::text_key("onboarding.intro"));
                ui.add_space(24.0);
                crate::ui::settings::card(ui, &theme::text_key("onboarding.hotkeyTitle"), |ui| {
                    ui.label(theme::text(
                        "使用 fcitx5 输入，并启用当前桌面的 OpenLess 组件。",
                    ));
                    ui.label(if openless_linux_egui::desktop_bridge::active() {
                        theme::text("桌面热键已连接")
                    } else {
                        theme::text("桌面组件尚未连接")
                    });
                    if ui.button(theme::text("安装并启用桌面组件")).clicked() {
                        self.desktop_component("install");
                    }
                    if ui.button(theme::text("重新启用")).clicked() {
                        self.desktop_component("enable");
                    }
                });
                crate::ui::settings::card(ui, &theme::text_key("onboarding.micTitle"), |ui| {
                    ui.label(theme::text_key("onboarding.micDesc"));
                    ui.label(format!(
                        "{} {}",
                        self.microphones.len(),
                        theme::text("个输入设备")
                    ));
                    if ui
                        .button(theme::text_key("onboarding.actionRequestMic"))
                        .clicked()
                    {
                        if let Some(backend) = self.backend() {
                            self.spawn(async move {
                                backend
                                    .services()
                                    .platform
                                    .request_microphone_permission()
                                    .await?;
                                Ok("麦克风权限已检查".into())
                            });
                        }
                        self.load_microphones();
                    }
                });
                ui.label(&self.status);
                if ui
                    .add_sized(
                        [ui.available_width(), 34.0],
                        egui::Button::new(theme::text_key("onboarding.continueToSettings")),
                    )
                    .clicked()
                {
                    match openless_linux_egui::save_ui_value(
                        "onboardingComplete",
                        serde_json::json!(true),
                    ) {
                        Ok(()) => {
                            ctx.data_mut(|d| d.insert_temp(id, true));
                            self.frontend_vm.settings_open = true;
                        }
                        Err(error) => self.status = error.to_string(),
                    }
                }
            });
    }
}
