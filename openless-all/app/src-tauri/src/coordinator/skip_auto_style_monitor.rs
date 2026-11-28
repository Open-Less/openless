//! Absolute replacement shared by settings changes and restore/rollback.
use openless_core::shared_types::ShortcutBinding;
use parking_lot::Mutex;

pub(super) fn replace<M, E>(
    slot: &Mutex<Option<M>>,
    binding: ShortcutBinding,
    install: impl FnOnce(ShortcutBinding) -> Result<M, E>,
) -> Result<(), E> {
    let mut slot = slot.lock();
    // Drop before registration, including an installation that fails. The
    // restore journal can then apply its old target through this same function.
    slot.take();
    if !binding.is_unassigned() && openless_core::legacy_modifier_trigger(&binding).is_none() {
        *slot = Some(install(binding)?);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeSet, sync::Arc};

    struct Registration {
        key: String,
        active: Arc<Mutex<BTreeSet<String>>>,
    }
    impl Drop for Registration {
        fn drop(&mut self) { assert!(self.active.lock().remove(&self.key)); }
    }
    fn combo(primary: &str) -> ShortcutBinding {
        ShortcutBinding { primary: primary.into(), modifiers: vec!["ctrl".into(), "shift".into()] }
    }
    fn apply(
        slot: &Mutex<Option<Registration>>, binding: ShortcutBinding,
        active: &Arc<Mutex<BTreeSet<String>>>,
    ) -> Result<(), &'static str> {
        replace(slot, binding, |binding| {
            let mut keys = active.lock();
            if !keys.insert(binding.primary.clone()) { return Err("registration conflict"); }
            Ok(Registration { key: binding.primary, active: active.clone() })
        })
    }

    #[test]
    fn skip_auto_style_absolute_restore_replaces_clears_and_rolls_back_registrations() {
        let active = Arc::new(Mutex::new(BTreeSet::new()));
        let slot = Mutex::new(None);
        apply(&slot, combo("A"), &active).unwrap();
        apply(&slot, combo("B"), &active).unwrap();
        assert_eq!(*active.lock(), BTreeSet::from(["B".into()]));
        apply(&slot, ShortcutBinding::unassigned(), &active).unwrap();
        assert!(active.lock().is_empty());
        apply(&slot, combo("A"), &active).unwrap();
        // Another action already owns B; failed restore must release A,
        // followed by the journal's absolute rollback to A.
        active.lock().insert("B".into());
        assert!(apply(&slot, combo("B"), &active).is_err());
        assert!(slot.lock().is_none());
        assert_eq!(*active.lock(), BTreeSet::from(["B".into()]));
        apply(&slot, combo("A"), &active).unwrap();
        assert_eq!(*active.lock(), BTreeSet::from(["A".into(), "B".into()]));
        apply(&slot, ShortcutBinding { primary: "LeftCommand".into(), modifiers: vec![] }, &active).unwrap();
        assert!(slot.lock().is_none(), "modifier taps do not keep a combo registration");
        assert_eq!(*active.lock(), BTreeSet::from(["B".into()]));
    }
}
