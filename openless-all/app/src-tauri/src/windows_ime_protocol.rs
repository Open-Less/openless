#![allow(dead_code, unused_imports, unused_variables)]
//! Wire format shared with `windows-ime/src/text_service.cpp`.
//!
//! OpenLess sends `WM_COPYDATA` to the message-only window the IME creates on
//! the host's TSF thread. A submit carries a token plus the text and is only
//! acknowledged; the commit result is then polled with query messages carrying
//! the same token. Replies travel back as the message result.
use serde::{Deserialize, Serialize};

pub const OPENLESS_IME_MESSAGE_WINDOW_CLASS: &str = "OpenLessImeMessageWindow";

/// `COPYDATASTRUCT::dwData` tags: "OLS1" (token + UTF-16LE text) and "OLQ1" (token).
pub const IME_COPYDATA_SUBMIT: usize = 0x4F4C_5331;
pub const IME_COPYDATA_QUERY: usize = 0x4F4C_5131;
pub const IME_MAX_SUBMIT_BYTES: usize = 1024 * 1024;

/// Replies are nonzero so they differ from an unhandled message (0). A failed
/// commit is reported as its HRESULT, which always has the high bit set.
pub const IME_STATUS_ACCEPTED: u32 = 0x4F4C_0001;
pub const IME_STATUS_PENDING: u32 = 0x4F4C_0002;
pub const IME_STATUS_COMMITTED: u32 = 0x4F4C_0003;
pub const IME_STATUS_UNKNOWN_TOKEN: u32 = 0x4F4C_0004;
pub const IME_STATUS_BAD_REQUEST: u32 = 0x4F4C_0005;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ImeSubmitStatus {
    Committed,
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
        ] {
            assert_ne!(status, 0);
            assert!(!is_failed_hresult(status));
        }
        assert!(is_failed_hresult(0x8000_4005));
    }
}
