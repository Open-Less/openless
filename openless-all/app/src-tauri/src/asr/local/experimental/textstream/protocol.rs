//! Pure private-worker protocol. No native libraries or process creation.
use openless_core::{BackendError, BackendErrorCode};
use serde::Deserialize;

pub const MAX_EVENT_BYTES: usize = 1_000_000;
fn error(message: impl std::fmt::Display) -> BackendError {
    BackendError::new(BackendErrorCode::Provider, message.to_string())
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Event {
    Ready {
        protocol: u32,
    },
    Stream {
        finalized: String,
        draft: String,
        sequence: u64,
    },
    Final {
        finalized: String,
        draft: String,
        sequence: u64,
    },
    Error {
        message: String,
    },
}
pub enum Update {
    Ready,
    Preview { finalized: String, draft: String },
    Finalized,
}
#[derive(Default)]
pub struct Decoder {
    ready: bool,
    sequence: u64,
    terminal: Option<String>,
}
impl Decoder {
    pub fn push(&mut self, line: &str) -> Result<Update, BackendError> {
        if line.len() > MAX_EVENT_BYTES {
            return Err(error("Oversized TextStream response"));
        }
        match serde_json::from_str::<Event>(line).map_err(error)? {
            Event::Ready { protocol: 1 } if !self.ready => {
                self.ready = true;
                Ok(Update::Ready)
            }
            Event::Stream {
                finalized,
                draft,
                sequence,
            } if self.ready && self.terminal.is_none() => {
                if sequence <= self.sequence {
                    return Err(error("Out-of-order TextStream snapshot"));
                }
                self.sequence = sequence;
                Ok(Update::Preview { finalized, draft })
            }
            Event::Final {
                finalized,
                draft,
                sequence,
            } if self.ready && self.terminal.is_none() => {
                if !draft.is_empty() || sequence <= self.sequence {
                    return Err(error("Invalid TextStream final snapshot"));
                }
                self.terminal = Some(finalized);
                Ok(Update::Finalized)
            }
            Event::Error { message } => Err(error(message)),
            _ => Err(error("Unexpected TextStream protocol event")),
        }
    }
    pub fn finish(self) -> Result<String, BackendError> {
        self.terminal
            .ok_or_else(|| error("TextStream ended without a finalized transcript"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ready() -> Decoder {
        let mut decoder = Decoder::default();
        decoder.push(r#"{"type":"ready","protocol":1}"#).unwrap();
        decoder
    }
    #[test]
    fn revisions_replace_unicode_previews_and_only_final_output_is_committed() {
        let mut decoder = ready();
        let mut text = openless_core::TranscriptAccumulator::default();
        for event in [
            r#"{"type":"stream","sequence":1,"finalized":"你好","draft":"啊，今天"}"#,
            r#"{"type":"stream","sequence":2,"finalized":"你好","draft":"！"}"#,
        ] {
            let Update::Preview { finalized, draft } = decoder.push(event).unwrap() else {
                panic!()
            };
            text.apply(&openless_core::TranscriptDelta {
                text: finalized + &draft,
                offset: 0,
                is_final: false,
            })
            .unwrap();
        }
        assert_eq!(text.text(), "你好！");
        decoder
            .push(r#"{"type":"final","sequence":3,"finalized":"你好，世界。","draft":""}"#)
            .unwrap();
        assert_eq!(decoder.finish().unwrap(), "你好，世界。");
    }
    #[test]
    fn eof_or_cancel_never_promotes_an_unconfirmed_draft() {
        let mut decoder = ready();
        decoder
            .push(r#"{"type":"stream","sequence":1,"finalized":"a","draft":"guess"}"#)
            .unwrap();
        assert!(decoder.finish().is_err());
    }
    #[test]
    fn rejects_invalid_handshake_order_and_terminal_drafts() {
        assert!(Decoder::default()
            .push(r#"{"type":"ready","protocol":2}"#)
            .is_err());
        let mut decoder = ready();
        let event = r#"{"type":"stream","sequence":1,"finalized":"a","draft":"b"}"#;
        decoder.push(event).unwrap();
        assert!(decoder.push(event).is_err());
        assert!(decoder
            .push(r#"{"type":"final","sequence":2,"finalized":"a","draft":"b"}"#)
            .is_err());
        assert!(decoder.push("not json").is_err());
    }
}
