//! Ask/Agent consumes one authorized reference as untrusted tool data.
//! The native binding and its verifier are supplied by the existing target owner.

use openless_core::{
    ask::AskTurnKey,
    assistant_api::AssistantContextResult,
    browser_scene::{AuthorizedBrowserReference, SceneAuthorization, BROWSER_REFERENCE_RULE},
};

pub(crate) fn consume<T>(
    reference: &mut AuthorizedBrowserReference<T>,
    key: AskTurnKey,
    authorization: SceneAuthorization,
    verify: impl FnOnce(&T) -> bool,
) -> AssistantContextResult {
    match reference.consume(key, authorization, verify) {
        Some(data) => AssistantContextResult::Available {
            // The reference is not interpolated into system/developer rules.
            // Its untrusted status is explicit even for nested tool consumers.
            data: serde_json::json!({
                "referenceType": "untrustedForegroundBrowserData",
                "handling": BROWSER_REFERENCE_RULE,
                "reference": data,
            }),
            source: "originalInputAccessibility".into(),
            as_of: chrono::Local::now().to_rfc3339(),
        },
        None => AssistantContextResult::Unavailable {
            reason: "browserReferenceExpired".into(),
        },
    }
}
