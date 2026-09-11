impl OpenLessEguiApp {
    fn history_transform(&mut self, retranscribe: bool) {
        if self.history_task.is_some() {return;}
        let (Some(backend),Some(entry))=(self.backend(),self.frontend_vm.history_entries.get(self.frontend_vm.history_selected).cloned()) else {return;};
        let tx=self.tx.clone();self.history_generation+=1;let generation=self.history_generation;
        let style=self.frontend_vm.history_repolish_style.clone();
        self.frontend_vm.history_busy=true;
        self.history_task=Some(self.tokio.spawn(async move {
            let id=entry.id.clone();
            let result:Result<String,BackendError>=async {
                if retranscribe {
                    let directory=backend.config().data_dir.clone();let recording=id.clone();
                    let pcm=tokio::task::spawn_blocking(move || {
                        let wav=openless_linux_egui::read_recording_wav(&directory,&recording).map_err(|e|BackendError::new(openless_core::BackendErrorCode::Persistence,e.to_string()))?;
                        openless_linux_egui::recording_pcm(&wav).map(|p|p.to_vec()).map_err(|e|BackendError::new(openless_core::BackendErrorCode::Persistence,e.to_string()))
                    }).await.map_err(|e|BackendError::new(openless_core::BackendErrorCode::Platform,e.to_string()))??;
                    let started=std::time::Instant::now();
                    let result=backend.services().auxiliary.retranscribe_pcm(pcm).await.map_err(|e|e.error)?;
                    let updated=backend.apply_history_retranscription(&id,result.text,&result.asr,started.elapsed().as_millis() as u64)?;
                    Ok(updated.final_text)
                } else {
                    backend.services().auxiliary.repolish(openless_core::RepolishRequest {
                        raw_text:entry.raw,style_pack_id:(!style.is_empty()).then_some(style),front_app:None,
                    }).await
                }
            }.await;
            let _=tx.send(UiResult::HistoryTransform {generation,id,repolish:!retranscribe,result:result.map_err(|e|e.to_string())});
        }));
    }
    fn history_confirmation_ui(&mut self, ctx: &egui::Context) {
        let Some(target)=self.history_confirmation.clone() else {return;};
        let response=egui::Modal::new(egui::Id::new("history-confirmation"))
            .frame(egui::Frame::new().fill(theme::surface()).corner_radius(14).inner_margin(24))
            .show(ctx,|ui| {
                ui.set_width(340.0);
                ui.heading(theme::text(if target.is_some() {"删除这条记录？"}else{"清空全部历史记录？"}));
                ui.label(theme::text("关联的录音也将删除。"));
                ui.add_space(20.0);
                ui.horizontal(|ui| {
                    if ui.button(theme::text("取消")).clicked() {self.history_confirmation=None;}
                    if ui.button(theme::text("确认删除")).clicked() {
                        self.history_confirmation=None;
                        if let Some(backend)=self.backend() {
                            let target=target.clone();let playback=self.playback.clone();
                            self.spawn(async move {
                                playback.stop();
                                let ids=if let Some(id)=&target {vec![id.clone()]}else{backend.list_history()?.into_iter().map(|e|e.id).collect()};
                                for id in ids {
                                    openless_linux_egui::remove_recording(&backend.config().data_dir,&id)
                                        .map_err(|e|BackendError::new(openless_core::BackendErrorCode::Persistence,e.to_string()))?;
                                    backend.delete_history(&id)?;
                                }
                                Ok("历史记录已删除".into())
                            });
                        }
                    }
                });
            });
        if response.should_close() {self.history_confirmation=None;}
    }
}
