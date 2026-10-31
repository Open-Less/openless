use anyhow::Result;
use serde::{Deserialize, Serialize};

const DESKTOP_ONBOARDING_FILE: &str = "desktop-onboarding.json";

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DesktopOnboardingState {
    pub finished: bool,
}

fn state_path() -> Result<std::path::PathBuf> {
    Ok(super::data_dir()?.join(DESKTOP_ONBOARDING_FILE))
}

pub fn desktop_onboarding_state() -> Result<DesktopOnboardingState> {
    super::read_or_default(&state_path()?)
}

pub fn set_desktop_onboarding_finished(finished: bool) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(&DesktopOnboardingState { finished })?;
    super::atomic_write(&state_path()?, &bytes)
}

#[cfg(test)]
mod tests {
    use super::DesktopOnboardingState;

    #[test]
    fn missing_state_defaults_to_unfinished() {
        assert!(!DesktopOnboardingState::default().finished);
    }
}
