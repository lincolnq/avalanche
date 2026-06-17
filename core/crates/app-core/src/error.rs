//! Application-level errors.

/// Internal error type with full Rust types.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("crypto error: {0}")]
    Crypto(#[from] crypto::CryptoError),

    #[error("store error: {0}")]
    Store(#[from] store::StoreError),

    #[error("network error: {0}")]
    Net(#[from] net::error::NetError),

    #[error("no account found in local store")]
    NoAccount,

    #[error("protocol error: {0}")]
    Protocol(String),

    #[error("contact is blocked: {0}")]
    Blocked(String),
}

/// UniFFI-exported error type. Flattened to strings since UniFFI can't
/// represent the inner error types from other crates.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum AppErrorFfi {
    #[error("{reason}")]
    Crypto { reason: String },

    #[error("{reason}")]
    Store { reason: String },

    #[error("{reason}")]
    Net { reason: String },

    #[error("no account found in local store")]
    NoAccount,

    #[error("{reason}")]
    Protocol { reason: String },

    #[error("{reason}")]
    Blocked { reason: String },
}

impl From<AppError> for AppErrorFfi {
    fn from(e: AppError) -> Self {
        match e {
            AppError::Crypto(e) => AppErrorFfi::Crypto { reason: e.to_string() },
            AppError::Store(e) => AppErrorFfi::Store { reason: e.to_string() },
            AppError::Net(e) => AppErrorFfi::Net { reason: e.to_string() },
            AppError::NoAccount => AppErrorFfi::NoAccount,
            AppError::Protocol(s) => AppErrorFfi::Protocol { reason: s },
            AppError::Blocked(s) => AppErrorFfi::Blocked { reason: s },
        }
    }
}
