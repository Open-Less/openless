//! The exact registration transaction used by the native page/tab observer.

use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Subject {
    Page,
    TabGroup,
}

pub(crate) struct Certificate(());

pub(crate) fn establish(
    mut register: impl FnMut(Subject, &'static str) -> bool,
) -> Option<Certificate> {
    for (subject, notification) in [
        (Subject::Page, "AXUIElementDestroyed"),
        (Subject::Page, "AXValueChanged"),
        (Subject::Page, "AXLayoutChanged"),
        (Subject::TabGroup, "AXSelectedChildrenChanged"),
        (Subject::TabGroup, "AXUIElementDestroyed"),
    ] {
        if !register(subject, notification) {
            return None;
        }
    }
    Some(Certificate(()))
}

pub(crate) fn transition(epoch: &AtomicU64) {
    epoch.fetch_add(1, Ordering::AcqRel);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_tab_group_subscription_never_creates_a_release_certificate() {
        let mut observed = Vec::new();
        let result = establish(|subject, event| {
            observed.push((subject, event));
            !(subject == Subject::TabGroup && event == "AXSelectedChildrenChanged")
        });
        assert!(result.is_none());
        assert_eq!(
            observed.last(),
            Some(&(Subject::TabGroup, "AXSelectedChildrenChanged"))
        );
        // No certificate can be recovered by an event or permission change.
        let epoch = AtomicU64::new(9);
        transition(&epoch);
        transition(&epoch);
        assert!(result.is_none());
        // A new original invocation must establish all required subscriptions.
        let mut registrations = 0;
        assert!(establish(|_, _| {
            registrations += 1;
            true
        })
        .is_some());
        assert_eq!(registrations, 5);
    }

    #[test]
    fn each_required_provider_failure_stops_the_registration_transaction() {
        for failure in 0..5 {
            let mut attempts = 0;
            assert!(establish(|_, _| {
                let accepted = attempts != failure;
                attempts += 1;
                accepted
            })
            .is_none());
            assert_eq!(attempts, failure + 1);
        }
    }
}
