//! Error type for the `store` crate.
//!
//! [`StoreError`] is the error type returned by all public methods on [`Store`].
//! It also implements `Into<libsignal_protocol::SignalProtocolError>` so that
//! the libsignal store trait implementations — which must return
//! `SignalProtocolError` — can convert store failures without boilerplate at
//! every call site.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Db(#[from] tokio_rusqlite::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("no local identity found; account not yet created")]
    NoIdentity,

    #[error("no registration found; device not yet registered")]
    NoRegistration,

    #[error("prekey {0} not found")]
    PreKeyNotFound(u32),

    #[error("signed prekey {0} not found")]
    SignedPreKeyNotFound(u32),

    #[error("data corruption: {0}")]
    Corrupt(String),
}

/// Convert a StoreError into a SignalProtocolError so store impls can
/// satisfy libsignal's trait bounds.
impl From<StoreError> for libsignal_protocol::SignalProtocolError {
    fn from(e: StoreError) -> Self {
        libsignal_protocol::SignalProtocolError::InvalidState(
            "store",
            e.to_string(),
        )
    }
}
