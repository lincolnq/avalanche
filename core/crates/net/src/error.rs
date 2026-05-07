//! Network client errors.

#[derive(Debug, thiserror::Error)]
pub enum NetError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("server returned {0}: {1}")]
    Server(u16, String),

    #[error("invalid base64: {0}")]
    Base64(String),

    #[error("WebSocket error: {0}")]
    WebSocket(String),
}
