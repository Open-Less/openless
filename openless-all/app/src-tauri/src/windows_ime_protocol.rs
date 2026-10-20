#![allow(dead_code, unused_imports, unused_variables)]
//! Wire format shared with `windows-ime/src/text_service.cpp`.
//!
//! OpenLess sends `WM_COPYDATA` to the message-only window the IME creates on
//! the host's TSF thread. A submit carries a token plus the text and is only
//! acknowledged; the commit result is then polled with query messages carrying
//! the same token. Replies travel back as the message result.
use serde::{Deserialize, Serialize};

pub const OPENLESS_IME_MESSAGE_WINDOW_CLASS: &str = "OpenLessImeMessageWindow";

/// `COPYDATASTRUCT::dwData` tags: "OLS1" (legacy token + UTF-16LE text), "OLS2" (token +
/// sender process + focus HWND + UTF-16LE text), "OLU1"/"OLF1"/"OLC1" (token + composition
/// owner + sender process + focus HWND + UTF-16LE text), "OLQ1" (token), and "OLV1" (empty
/// protocol-version probe).
///
/// OLS2 is deliberately a new tag: an old in-host DLL must reject it rather than interpret the
/// recovery metadata as text and report a false successful commit.
pub const IME_COPYDATA_SUBMIT: usize = 0x4F4C_5332;
pub const IME_COPYDATA_QUERY: usize = 0x4F4C_5131;
/// "OLU1": replace the live composition with the transcript so far (starting one if needed).
pub const IME_COPYDATA_UPDATE: usize = 0x4F4C_5531;
/// "OLF1": replace the live composition with the final text and end it.
pub const IME_COPYDATA_FINISH: usize = 0x4F4C_4631;
/// "OLC1": restore the original selection and cancel the live composition.
pub const IME_COPYDATA_CANCEL: usize = 0x4F4C_4331;
/// "OLV1": a zero-length protocol probe used to distinguish the current DLL from a stale
/// in-process registration before sending a composition payload.
pub const IME_COPYDATA_VERSION: usize = 0x4F4C_5631;

/// What one submit does in the host document. Mirrors `OpenLessEditOp` in the DLL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImeSubmitOp {
    /// Insert the text at the selection (the classic dictation commit).
    #[default]
    Commit,
    /// Show the text as an underlined composition, replacing the previous update.
    Update,
    /// Replace the composition with the final text and end it.
    Finish,
    /// Restore the original selection and end the composition without committing.
    Cancel,
}

impl ImeSubmitOp {
    pub fn copydata_tag(self) -> usize {
        match self {
            Self::Commit => IME_COPYDATA_SUBMIT,
            Self::Update => IME_COPYDATA_UPDATE,
            Self::Finish => IME_COPYDATA_FINISH,
            Self::Cancel => IME_COPYDATA_CANCEL,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Commit => "commit",
            Self::Update => "update",
            Self::Finish => "finish",
            Self::Cancel => "cancel",
        }
    }
}
pub const IME_MAX_SUBMIT_BYTES: usize = 1024 * 1024;

/// Replies are nonzero so they differ from an unhandled message (0). A failed
/// commit is reported as its HRESULT, which always has the high bit set.
pub const IME_STATUS_ACCEPTED: u32 = 0x4F4C_0001;
pub const IME_STATUS_PENDING: u32 = 0x4F4C_0002;
pub const IME_STATUS_COMMITTED: u32 = 0x4F4C_0003;
pub const IME_STATUS_UNKNOWN_TOKEN: u32 = 0x4F4C_0004;
pub const IME_STATUS_BAD_REQUEST: u32 = 0x4F4C_0005;
/// The edit request was rejected before TSF dispatched the edit session. No native composition
/// can have been opened, so callers may use their configured non-TSF fallback.
pub const IME_STATUS_NOT_DISPATCHED: u32 = 0x4F4C_0006;

/// The request was rejected because another insertion still owns a live or externally terminated
/// composition. Fallback must not replay text beside that composition.
pub const IME_STATUS_COMPOSITION_CONFLICT: u32 = 0x4F4C_0007;
/// The native service has an orphaned composition from a previous OpenLess process; the caller
/// must cancel it and retry the update against the current focus.
pub const IME_STATUS_COMPOSITION_RECOVERY_REQUIRED: u32 = 0x4F4C_0008;
/// The installed DLL answered the protocol probe and reports the current wire format.
pub const IME_STATUS_PROTOCOL_VERSION: u32 = 0x4F4C_0009;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ImeSubmitStatus {
    Committed,
    NotDispatched,
    CompositionConflict,
    CompositionRecoveryRequired,
    Rejected,
    Failed,
}

pub fn is_failed_hresult(reply: u32) -> bool {
    reply & 0x8000_0000 != 0
}

pub fn encode_submit_payload(token: u32, text: &str) -> Vec<u8> {
    let mut payload = Vec::with_capacity(4 + text.len() * 2);
    payload.extend_from_slice(&token.to_le_bytes());
    for unit in text.encode_utf16() {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    payload
}

/// Plain commits carry the sender process and captured focus HWND too, so the native service can
/// distinguish an orphaned composition from another live insertion in the same process.
pub fn encode_commit_submit_payload(
    token: u32,
    sender_process: u32,
    target_window: u64,
    text: &str,
) -> Vec<u8> {
    let mut payload = Vec::with_capacity(16 + text.len() * 2);
    payload.extend_from_slice(&token.to_le_bytes());
    payload.extend_from_slice(&sender_process.to_le_bytes());
    payload.extend_from_slice(&target_window.to_le_bytes());
    for unit in text.encode_utf16() {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    payload
}

/// Composition submits carry a stable owner and captured focus window after the per-request query
/// token. The native service rejects a later dictation from reusing an abandoned range or priming
/// against a changed focus.
pub fn encode_composition_submit_payload(
    token: u32,
    owner: u32,
    sender_process: u32,
    target_window: u64,
    text: &str,
) -> Vec<u8> {
    let mut payload = Vec::with_capacity(20 + text.len() * 2);
    payload.extend_from_slice(&token.to_le_bytes());
    payload.extend_from_slice(&owner.to_le_bytes());
    payload.extend_from_slice(&sender_process.to_le_bytes());
    payload.extend_from_slice(&target_window.to_le_bytes());
    for unit in text.encode_utf16() {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    payload
}

pub fn encode_query_payload(token: u32) -> [u8; 4] {
    token.to_le_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submit_payload_is_token_followed_by_utf16le_text() {
        assert_eq!(
            encode_submit_payload(0x0403_0201, "A\u{4f60}"),
            vec![0x01, 0x02, 0x03, 0x04, 0x41, 0x00, 0x60, 0x4f]
        );
    }

    #[test]
    fn submit_payload_keeps_surrogate_pairs() {
        let payload = encode_submit_payload(1, "\u{1F600}");
        assert_eq!(payload.len(), 4 + 4);
        assert_eq!(&payload[4..], &[0x3D, 0xD8, 0x00, 0xDE]);
    }

    #[test]
    fn commit_payload_carries_sender_process_and_focus_after_token() {
        assert_eq!(
            encode_commit_submit_payload(7, 9, 0x0102_0304_0506_0708, "A"),
            vec![
                7, 0, 0, 0, 9, 0, 0, 0, 8, 7, 6, 5, 4, 3, 2, 1, 0x41, 0x00,
            ]
        );
    }

    #[test]
    fn composition_payload_carries_a_stable_owner_after_the_query_token() {
        assert_eq!(
            encode_composition_submit_payload(7, 9, 11, 0x0102_0304_0506_0708, "A"),
            vec![
                7, 0, 0, 0, 9, 0, 0, 0, 11, 0, 0, 0, 8, 7, 6, 5, 4, 3, 2, 1, 0x41, 0x00,
            ]
        );
    }

    #[test]
    fn empty_text_still_carries_the_token() {
        assert_eq!(encode_submit_payload(7, ""), vec![7, 0, 0, 0]);
        assert_eq!(encode_query_payload(7), [7, 0, 0, 0]);
    }

    #[test]
    fn status_codes_never_look_like_failed_hresults() {
        for status in [
            IME_STATUS_ACCEPTED,
            IME_STATUS_PENDING,
            IME_STATUS_COMMITTED,
            IME_STATUS_UNKNOWN_TOKEN,
            IME_STATUS_BAD_REQUEST,
            IME_STATUS_NOT_DISPATCHED,
            IME_STATUS_COMPOSITION_CONFLICT,
            IME_STATUS_COMPOSITION_RECOVERY_REQUIRED,
            IME_STATUS_PROTOCOL_VERSION,
        ] {
            assert_ne!(status, 0);
            assert!(!is_failed_hresult(status));
        }
        assert!(is_failed_hresult(0x8000_4005));
    }
}
