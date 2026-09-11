impl OpenLessEguiApp {
    fn native_windows(&mut self, ctx: &egui::Context) {
        use openless_core::SelectionVoicePhase as Phase;
        if let Some(snapshot) = self
            .selection_voice_state
            .clone()
            .filter(|s| matches!(s.phase, Phase::AwaitingIntent | Phase::Preview))
        {
            let intent = snapshot.phase == Phase::AwaitingIntent;
            let title = if intent {
                "OpenLess Voice Intent"
            } else {
                "OpenLess Voice Preview"
            };
            let placement=egui::Id::new(("voice-placement",snapshot.session_id,intent));
            if !ctx.data(|d|d.get_temp::<bool>(placement).unwrap_or(false)) {
                openless_linux_egui::desktop_bridge::place_popup(title,480,320,false);
                ctx.data_mut(|d|d.insert_temp(placement,true));
            }
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("selection-voice"),
                egui::ViewportBuilder::default()
                    .with_title(title)
                    .with_inner_size([480.0, 320.0])
                    .with_always_on_top(),
                |ctx, _class| {
                    theme::apply_visuals(
                        ctx,
                        self.preferences
                            .as_ref()
                            .map(|p| p.theme_mode)
                            .unwrap_or_default(),
                    );
                    let close = ctx.input(|i| {
                        i.viewport().close_requested() || i.key_pressed(egui::Key::Escape)
                    });
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.add_space(12.0);
                        ui.heading(if intent {
                            theme::text("你想对选中文字做什么？")
                        } else {
                            theme::text("预览修改")
                        });
                        ui.add_space(12.0);
                        ui.label(snapshot.source_text.as_deref().unwrap_or_default());
                        let driver = self
                            .native
                            .as_ref()
                            .map(|native| native.host().selection_voice());
                        let mut cancel = close;
                        if intent {
                            ui.label(
                                snapshot
                                    .instruction_polished
                                    .as_deref()
                                    .or(snapshot.instruction_raw.as_deref())
                                    .unwrap_or_default(),
                            );
                            ui.horizontal(|ui| {
                                for (intent, label) in [("question", "追问"), ("edit", "修改")]
                                {
                                    if ui.button(theme::text(label)).clicked() {
                                        if let (Some(driver), Some(id)) =
                                            (driver.clone(), snapshot.session_id)
                                        {
                                            self.spawn(async move {
                                                driver.confirm_intent(id, intent.into()).await?;
                                                Ok("已选择语音意图".into())
                                            });
                                        }
                                    }
                                }
                            });
                        } else if let Some(preview) = &snapshot.preview {
                            let id =
                                egui::Id::new(("voice-preview", preview.session_id.to_string()));
                            let mut text = ctx
                                .data(|data| data.get_temp::<String>(id))
                                .unwrap_or_else(|| preview.text.clone());
                            ui.add(
                                egui::TextEdit::multiline(&mut text)
                                    .desired_rows(6)
                                    .desired_width(f32::INFINITY),
                            );
                            ctx.data_mut(|data| data.insert_temp(id, text.clone()));
                            if ui.button(theme::text("应用修改")).clicked() {
                                if let Some(driver) = driver.clone() {
                                    let owner = preview.owner_session_id;
                                    self.spawn(async move {
                                        driver.apply(text, owner).await?;
                                        Ok("已应用修改".into())
                                    });
                                }
                            }
                            if preview.can_revert && ui.button(theme::text("撤回")).clicked() {
                                if let Some(backend) = self.backend() {
                                    let owner = preview.owner_session_id;
                                    self.spawn(async move {
                                        backend.services().selection_voice.revert_preview(owner)?;
                                        Ok("已撤回到上一版预览".into())
                                    });
                                }
                            }
                        }
                        cancel |= ui.button(theme::text("取消")).clicked();
                        if cancel {
                            if let (Some(driver), Some(id)) = (driver, snapshot.session_id) {
                                self.spawn(async move {
                                    driver.cancel(id).await?;
                                    Ok("已取消".into())
                                });
                            }
                            self.selection_voice_state = None;
                        }
                    });
                },
            );
        }
        if let Some(driver)=self.native.as_ref().map(|n|n.host().selection_voice()) {
            if let Some(id)=driver.applied_target() {
                egui::Window::new(theme::text("修改已应用")).id(egui::Id::new("voice-applied"))
                    .anchor(egui::Align2::RIGHT_BOTTOM,[-20.0,-20.0]).resizable(false).collapsible(false).show(ctx,|ui| {
                        ui.label(theme::text("撤回前将核验原应用和已写入的文本。"));
                        ui.horizontal(|ui| {
                            if ui.button(theme::text("撤回修改")).clicked() {
                                let driver=driver.clone();self.spawn(async move {driver.revert_applied(id).await?;Ok("修改已撤回".into())});
                            }
                            if ui.button(theme::text("完成")).clicked() {driver.dismiss_applied();}
                        });
                    });
            }
        }
        if self.less_computer_visible {
            let placement=egui::Id::new("less-computer-placement");
            if !ctx.data(|d|d.get_temp::<bool>(placement).unwrap_or(false)) {
                openless_linux_egui::desktop_bridge::place_popup("OpenLess Less Computer",520,600,false);
                ctx.data_mut(|d|d.insert_temp(placement,true));
            }
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("less-computer-panel"),
                egui::ViewportBuilder::default()
                    .with_title("OpenLess Less Computer")
                    .with_inner_size([520.0, 600.0])
                    .with_always_on_top(),
                |ctx, _class| {
                    theme::apply_visuals(
                        ctx,
                        self.preferences
                            .as_ref()
                            .map(|p| p.theme_mode)
                            .unwrap_or_default(),
                    );
                    if ctx.input(|i| i.viewport().close_requested()) {
                        self.less_computer_visible = false;
                        ctx.data_mut(|d|d.remove::<bool>(egui::Id::new("less-computer-placement")));
                    }
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let t = ui.input(|i| i.time) as f32;
                        let glow = if self.pending_approval.is_some() {
                            egui::Color32::from_rgb(217, 119, 6)
                        } else {
                            theme::blue()
                        };
                        let rect = ui.max_rect().shrink(2.0);
                        ui.painter().rect_stroke(
                            rect,
                            14,
                            egui::Stroke::new(1.5 + 0.5 * (t * 2.0).sin(), glow),
                            egui::StrokeKind::Inside,
                        );
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            self.less_computer_ui(ui);
                        });
                    });
                },
            );
        }
    }
}
