#[derive(Default)]
struct MarketplaceUi {
    generation:u64,
    loading:bool,
    search_due:Option<std::time::Instant>,
    mine_open:bool,
    busy:bool,
    upload_pack:String,
    confirmation:Option<(MarketplaceMutation,String)>,
}
#[derive(Clone)]
enum MarketplaceMutation {Like(String),Upload(String,Option<String>),Delete(String)}
impl OpenLessEguiApp {
    fn load_marketplace(&mut self) {
        let Some(backend)=self.backend() else {return;};
        self.marketplace_ui.generation+=1;
        self.marketplace_ui.loading=true;self.marketplace_ui.search_due=None;
        self.frontend_vm.marketplace_selected=None;
        let generation=self.marketplace_ui.generation;
        let query=self.marketplace_query.trim().to_string();let sort=self.frontend_vm.marketplace_sort;
        let tx=self.tx.clone();
        self.tokio.spawn(async move {
            let result:Result<_,BackendError>=async {
                use frontend::view_model::MarketplaceSort;
                let mut items=backend.services().marketplace.list(openless_core::MarketplaceQuery {
                    query:(!query.is_empty()).then_some(query),limit:Some(100),
                    sort:Some(if sort==MarketplaceSort::New {"new"}else{"popular"}.into()),
                }).await?;
                let signed_in=backend.services().marketplace.auth_status().await?.signed_in;
                let likes=if signed_in {backend.services().marketplace.my_likes().await?}else{vec![]};
                if sort==MarketplaceSort::Liked {items.retain(|item|likes.contains(&item.id));}
                Ok((items,likes))
            }.await;
            let _=tx.send(UiResult::Marketplace{generation,result:result.map_err(|e|e.to_string())});
        });
    }
    fn mutate_marketplace(&mut self, operation:MarketplaceMutation) {
        if self.marketplace_ui.busy {return;}
        let Some(backend)=self.backend() else {return;};
        self.marketplace_ui.busy=true;let tx=self.tx.clone();
        self.tokio.spawn(async move {
            let result:Result<String,BackendError>=async {
                match operation {
                    MarketplaceMutation::Like(id)=>{let result=backend.services().marketplace.toggle_like(id).await?;Ok(format!("{} 个赞",result.like_count))}
                    MarketplaceMutation::Upload(id,origin)=>{let result=backend.services().marketplace.upload(id,origin).await?;Ok(result.message)}
                    MarketplaceMutation::Delete(id)=>{backend.services().marketplace.delete(id).await?;Ok("已删除发布".into())}
                }
            }.await;
            let _=tx.send(UiResult::MarketplaceMutation(result.map_err(|e|e.to_string())));
        });
    }
    fn marketplace_windows_v2(&mut self,ctx:&egui::Context) {
        if self.marketplace_ui.search_due.is_some_and(|due|std::time::Instant::now()>=due){self.load_marketplace();}
        if self.marketplace_ui.mine_open {
            let mut open=true;
            egui::Window::new(theme::text("我的发布")).open(&mut open).default_width(600.0).show(ctx,|ui| {
                self.marketplace_account_v2(ui);
                ui.separator();
                egui::ComboBox::from_id_salt("upload-style").selected_text(self.style_packs.iter()
                    .find(|p|p.id==self.marketplace_ui.upload_pack).map(|p|p.name.as_str()).unwrap_or("选择本地风格包"))
                    .show_ui(ui,|ui| {for pack in &self.style_packs {ui.selectable_value(&mut self.marketplace_ui.upload_pack,pack.id.clone(),&pack.name);}});
                if ui.add_enabled(!self.marketplace_ui.busy && !self.marketplace_ui.upload_pack.is_empty(),egui::Button::new(theme::text("发布风格包"))).clicked() {
                    if let Some(pack)=self.style_packs.iter().find(|p|p.id==self.marketplace_ui.upload_pack) {
                        self.marketplace_ui.confirmation=Some((MarketplaceMutation::Upload(pack.id.clone(),pack.origin_pack_id.clone()),format!("发布「{}」到风格市场？提示词及风格信息将公开。",pack.name)));
                    }
                }
                ui.add_space(12.0);
                egui::ScrollArea::vertical().max_height(320.0).show(ui,|ui| {
                    for pack in &self.marketplace_my_packs {
                        ui.horizontal(|ui| {
                            ui.strong(&pack.summary.name);ui.label(&pack.state);
                            if ui.add_enabled(!self.marketplace_ui.busy,egui::Button::new(theme::text("删除"))).clicked(){
                                self.marketplace_ui.confirmation=Some((MarketplaceMutation::Delete(pack.summary.id.clone()),format!("从市场删除「{}」？",pack.summary.name)));
                            }
                        });ui.separator();
                    }
                });
                if self.marketplace_ui.busy {ui.spinner();}
                ui.label(&self.status);
            });
            if !open {self.marketplace_ui.mine_open=false;}
        }
        if let Some((operation,message))=self.marketplace_ui.confirmation.clone() {
            let response=egui::Modal::new(egui::Id::new("marketplace-confirmation")).show(ctx,|ui| {
                ui.set_width(360.0);ui.label(message);ui.add_space(16.0);
                ui.horizontal(|ui| {
                    if ui.button(theme::text("取消")).clicked(){self.marketplace_ui.confirmation=None;}
                    if ui.button(theme::text("确认")).clicked(){self.marketplace_ui.confirmation=None;self.mutate_marketplace(operation);}
                });
            });
            if response.should_close(){self.marketplace_ui.confirmation=None;}
        }
    }
}
