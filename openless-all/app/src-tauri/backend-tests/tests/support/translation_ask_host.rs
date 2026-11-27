//! The production Ask keyboard controller runs against inert desktop effects.
//! Native::key_edge, pairing, physical modifiers and runtime routing are real.
use std::{future::Future, pin::Pin, time::Duration};
use openless_core::DictationHotkeyEdge;

#[path = "../../../src/ask_window/hotkey.rs"]
pub(crate) mod hotkey;

pub fn active_space_epoch() -> u64 { 0 }
pub fn foreground_pid() -> Option<u32> { None }
pub fn install_test_runtime(desktop: &std::sync::Arc<Desktop>) {
    hotkey::install(std::sync::Arc::downgrade(desktop));
}

pub mod capture {
    pub struct Foreground { pub pid: u32 }
    pub struct Trigger;
    impl Trigger {
        pub fn at_key_event(_: Option<Foreground>) -> Self { Self }
    }
    pub fn foreground() -> Option<Foreground> { None }
}
pub mod voice {
    use super::*;
    #[derive(Clone)]
    pub struct Target;
    #[derive(Default)]
    pub struct Controller;
    impl Controller {
        pub fn manual(&self, _: Option<Target>, _: u64, _: bool, _: Option<hotkey::FnCommit>) {}
        pub fn release(&self, _: u64, _: Duration, _: bool, _: bool) {}
        pub fn cancel_press(&self, _: u64) {}
    }
}

#[derive(Default)]
pub struct Ports;
impl Ports {
    pub fn ordinary_dictation_is_active(&self) -> bool { true }
    pub fn ordinary_dictation_edge(
        &self, _: DictationHotkeyEdge, _: Option<hotkey::FnCommit>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send>> { Box::pin(async {}) }
}
#[derive(Default)]
pub struct Desktop {
    pub ports: Ports,
    pub voice: voice::Controller,
}
impl Desktop {
    pub fn is_native_target(&self, _: Option<&capture::Foreground>) -> bool { false }
    pub fn target_at_press(&self) -> Option<voice::Target> { None }
    pub fn voice_is_pending(&self) -> bool { false }
    pub fn begin_open(&self) -> u64 { 1 }
    pub fn cancel_pending_open(&self) {}
    pub fn is_focused(&self) -> bool { false }
    pub fn warm_dictation(&self) {}
    pub fn dismiss_composer_from_escape(&self) {}
    pub async fn open_with(
        &self, _: Option<capture::Trigger>, _: Option<bool>, _: u64,
    ) -> Result<(), String> { Ok(()) }
}
