//! Mobile stub — unicode keystroke streaming is unavailable on mobile.

use tauri::{AppHandle, Runtime};

#[derive(Debug, thiserror::Error)]
pub enum TypeError {
    #[allow(dead_code)]
    #[error("{source} after {typed_chars} chars were sent")]
    Partial {
        typed_chars: usize,
        #[source]
        source: Box<TypeError>,
    },
    #[error("unicode keystroke unavailable on mobile")]
    Unavailable,
}

impl TypeError {
    pub fn typed_chars(&self) -> usize {
        match self {
            TypeError::Partial { typed_chars, .. } => *typed_chars,
            _ => 0,
        }
    }
}

pub async fn switch_to_ascii<R: Runtime>(_app: &AppHandle<R>) -> Result<Option<()>, TypeError> {
    Err(TypeError::Unavailable)
}

pub async fn restore_input_source<R: Runtime>(
    _app: &AppHandle<R>,
    _previous: Option<()>,
) -> Result<(), TypeError> {
    Ok(())
}

pub fn type_unicode_chunk(_text: &str) -> Result<usize, TypeError> {
    Err(TypeError::Unavailable)
}
