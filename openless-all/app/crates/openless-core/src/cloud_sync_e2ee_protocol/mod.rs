//! Native-only encrypted sync wire protocol. Never expose these secret-bearing
//! types through UI status/events. Scheduling, persistence and restore belong to
//! the caller; this module never retries a write or applies a downloaded document.

pub(crate) mod crypto;
pub(crate) mod transport;
pub mod types;

pub(crate) type Result<T> = std::result::Result<T, Error>;

/// Only fixed diagnostic text and validated protocol codes may cross this boundary.
/// In particular, no reqwest/serde error, server message, token or payload is kept.
#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("invalid encrypted sync endpoint")]
    InvalidEndpoint,
    #[error("invalid encrypted sync response: {0}")]
    InvalidResponse(&'static str),
    #[error("invalid encrypted sync data: {0}")]
    InvalidWire(&'static str),
    #[error("encrypted sync transport failed; a write may have committed")]
    Transport,
    #[error("encrypted sync payload exceeds the protocol limit")]
    PayloadTooLarge,
    #[error("encrypted sync account does not match")]
    AccountMismatch,
    #[error("encrypted sync version or key context does not match")]
    ContextMismatch,
    #[error("unsupported encrypted sync protocol")]
    UnsupportedProtocol,
    #[error("unsupported encrypted document version")]
    UnsupportedDocumentVersion,
    #[error("encryption password does not meet the local password policy")]
    WeakPassword,
    #[error("normalized encryption passwords do not match")]
    PasswordConfirmationMismatch,
    #[error("invalid encryption password or ciphertext")]
    InvalidPasswordOrCiphertext,
    #[error("secure random source unavailable")]
    RandomUnavailable,
    #[error("encrypted sync revision exhausted")]
    RevisionExhausted,
    #[error("encrypted sync service rejected the request ({status}, {code})")]
    Api {
        status: u16,
        code: types::RemoteErrorCode,
        retry_after_seconds: Option<u32>,
        current_revision: Option<types::Revision>,
    },
}
