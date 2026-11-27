use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use openless_core::{BackendError, HostAction, HostActions};

/// Thread-safe bridge from core semantic actions to the Linux UI event loop.
///
/// A host may install a wake callback that calls the windowing system's wake or
/// repaint primitive.  Draining is non-blocking and never calls the UI directly.
#[derive(Default)]
pub struct LinuxHostActions {
    pending: Mutex<VecDeque<HostAction>>,
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl LinuxHostActions {
    pub fn new(wake: Option<Arc<dyn Fn() + Send + Sync>>) -> Self {
        Self {
            pending: Mutex::new(VecDeque::new()),
            wake,
        }
    }

    /// 设置唤醒回调：core 每推一个 action 就调用一次，宿主据此立即 drain，
    /// 不需要定时轮询。
    pub fn set_wake(&mut self, wake: Arc<dyn Fn() + Send + Sync>) {
        self.wake = Some(wake);
    }

    pub fn drain(&self, mut apply: impl FnMut(HostAction)) -> usize {
        let actions = {
            let mut pending = self.pending.lock().expect("host action queue poisoned");
            pending.drain(..).collect::<Vec<_>>()
        };
        let count = actions.len();
        for action in actions {
            apply(action);
        }
        count
    }

    pub fn len(&self) -> usize {
        self.pending
            .lock()
            .expect("host action queue poisoned")
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl HostActions for LinuxHostActions {
    fn request(&self, action: HostAction) -> Result<(), BackendError> {
        self.pending
            .lock()
            .expect("host action queue poisoned")
            .push_back(action);
        if let Some(wake) = &self.wake {
            wake();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_are_drained_in_request_order() {
        let actions = LinuxHostActions::default();
        actions.request(HostAction::ShowMain).unwrap();
        actions.request(HostAction::FocusMain).unwrap();
        let mut drained = Vec::new();
        assert_eq!(actions.drain(|action| drained.push(action)), 2);
        assert_eq!(drained, vec![HostAction::ShowMain, HostAction::FocusMain]);
        assert!(actions.is_empty());
    }
}
