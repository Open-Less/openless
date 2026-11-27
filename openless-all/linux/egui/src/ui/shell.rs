//! Shell page identity.
//!
//! The egui shell used to live here (titlebar / sidebar / content panel). All of
//! that was replaced by `ui::frontend`; only the page identity remains because
//! the host keeps its own navigation state and maps it onto the frontend pages.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Overview,
    History,
    QuickNote,
    Vocabulary,
    Styles,
    Marketplace,
    Providers,
    Assistant,
    Translation,
    Corrections,
}
