//! Host lifecycle changes must revoke preparation as well as accepted turns.

use openless_core::ask::{AskSessionFence, StandaloneQaService};
use openless_core::ask_context::ContextAuthorization;
use uuid::Uuid;

use super::context::DesktopContext;

pub(super) fn new_persistent_conversation(
    service: &StandaloneQaService,
    fence: &AskSessionFence,
) -> Uuid {
    fence.revoke();
    // Keep old conversations: a claimed native operation still needs to report
    // its outcome, including Unknown, and persistent history remains available.
    service.cancel_active_turns();
    service.new_conversation()
}

pub(super) fn revoke_app_context(
    context: &DesktopContext,
    service: &StandaloneQaService,
    fence: &AskSessionFence,
) {
    context.set_authorization(ContextAuthorization::default());
    fence.revoke();
    service.cancel_active_turns();
}
