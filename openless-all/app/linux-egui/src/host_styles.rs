impl OpenLessEguiApp {
    fn open_style_v2(&mut self, pack: openless_core::StylePack) {
        let vm=&mut self.frontend_vm;
        vm.style_editor_open=true; vm.style_saving=false;
        vm.style_name=pack.name.clone(); vm.style_description=pack.description.clone();
        vm.style_prompt=pack.prompt.clone(); vm.style_selection_prompt=pack.selection_prompt.clone();
        vm.style_builtin=pack.kind==openless_core::StylePackKind::Builtin;
        self.style_editor=Some(pack);
    }
    fn activate_style_v2(&mut self, index: usize) {
        let id=if index==usize::MAX {"builtin.raw".into()}else{
            let Some(pack)=self.style_packs.get(index) else {return;}; pack.id.clone()
        };
        if self.frontend_vm.style_selection_workflow {
            self.save_field_edits(std::collections::BTreeMap::from([("/selectionPolishStylePackId".into(),serde_json::json!(id))]));
        } else if let Some(backend)=self.backend() {
            self.spawn(async move {backend.activate_style_pack(&id)?; Ok("风格已切换".into())});
        }
    }
    fn save_style_v2(&mut self, prompt: String) {
        let Some(mut pack)=self.style_editor.clone() else {self.frontend_vm.style_saving=false;return;};
        let Some(backend)=self.backend() else {self.frontend_vm.style_saving=false;return;};
        pack.name=self.frontend_vm.style_name.trim().to_string();
        pack.description=self.frontend_vm.style_description.clone();
        pack.prompt=prompt; pack.selection_prompt=self.frontend_vm.style_selection_prompt.clone();
        let exists=self.style_packs.iter().any(|p|p.id==pack.id);
        let tx=self.tx.clone();
        self.tokio.spawn(async move {
            let result=tokio::task::spawn_blocking(move || {
                if exists {backend.update_style_pack(pack)}else{backend.create_style_pack(pack)}
            }).await.map_err(|e|e.to_string()).and_then(|r|r.map_err(|e|e.to_string()));
            let _=tx.send(UiResult::StyleSaved(result));
        });
    }
    fn reset_style_v2(&mut self) {
        let (Some(pack),Some(backend))=(self.style_editor.as_ref(),self.backend()) else {return;};
        let id=pack.id.clone(); let tx=self.tx.clone(); self.frontend_vm.style_saving=true;
        self.tokio.spawn(async move {
            let result=tokio::task::spawn_blocking(move ||backend.reset_builtin_style_pack(&id)).await
                .map_err(|e|e.to_string()).and_then(|r|r.map_err(|e|e.to_string()));
            let _=tx.send(UiResult::StyleSaved(result));
        });
    }
    fn style_delete_confirmation_ui(&mut self, ctx:&egui::Context) {
        let Some(id)=self.style_delete_pending.clone() else {return;};
        let response=egui::Modal::new(egui::Id::new("delete-style"))
            .frame(egui::Frame::new().fill(theme::surface()).corner_radius(14).inner_margin(24))
            .show(ctx,|ui| {
                ui.set_width(340.0);ui.heading(theme::text("删除风格包？"));
                ui.label(theme::text("此操作会同时解除关联的快捷键。"));ui.add_space(16.0);
                ui.horizontal(|ui| {
                    if ui.button(theme::text("取消")).clicked(){self.style_delete_pending=None;}
                    if ui.button(theme::text("删除")).clicked(){
                        self.style_delete_pending=None;
                        let exists=self.style_packs.iter().any(|p|p.id==id);
                        if exists {
                            if let Some(native)=&self.native {
                                let host=native.host_arc();
                                self.spawn(async move {host.remove_style_pack(&id)?;Ok("风格包已删除".into())});
                            }
                        }
                        self.frontend_vm.style_editor_open=false;self.style_editor=None;
                    }
                });
            });
        if response.should_close(){self.style_delete_pending=None;}
    }
}
