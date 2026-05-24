//! Server configuration loaded from environment variables.
//!
//! All values have sensible defaults for local development. In production,
//! operators set environment variables to override them. The `DATABASE_URL`
//! default points at the Docker Compose Postgres instance.

/// Server configuration, loaded from environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    /// PostgreSQL connection string.
    pub database_url: String,
    /// Address to bind the HTTP server to.
    pub bind_addr: String,
    /// The public URL of this homeserver (used in DID documents).
    pub server_url: String,
    /// Session token lifetime in seconds (default: 24 hours).
    pub token_lifetime_secs: i64,
    /// Message expiry in seconds (default: 30 days).
    pub message_expiry_secs: i64,
    /// Minimum allowed per-message expiry in seconds (default: 5 minutes).
    pub message_expiry_min_secs: i64,
    /// Maximum allowed per-message expiry in seconds (default: 30 days).
    pub message_expiry_max_secs: i64,
    /// Prekey pool low-water mark (default: 10).
    pub prekey_low_threshold: i64,
    /// Project token lifetime in seconds (default: 1 hour).
    pub project_token_lifetime_secs: i64,
    /// Installed Projects as JSON array: [{"name":"...","url":"...","description":"..."}].
    pub projects_json: String,
    /// Push relay URL (e.g. "http://localhost:3002"). If unset, push is disabled.
    pub relay_url: Option<String>,
    /// Human-readable server name (shown to users during invite/onboarding).
    pub server_name: String,
    /// Domain used for deep link URLs in invite redirects (default: go.theavalanche.net).
    pub invite_domain: String,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            database_url: std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "postgres://actnet:actnet-dev@localhost/actnet".to_string()),
            bind_addr: std::env::var("BIND_ADDR")
                .unwrap_or_else(|_| "0.0.0.0:3000".to_string()),
            server_url: std::env::var("SERVER_URL")
                .unwrap_or_else(|_| "http://localhost:3000".to_string()),
            token_lifetime_secs: std::env::var("TOKEN_LIFETIME_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(86400),
            message_expiry_secs: std::env::var("MESSAGE_EXPIRY_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30 * 86400),
            message_expiry_min_secs: std::env::var("MESSAGE_EXPIRY_MIN_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(300),
            message_expiry_max_secs: std::env::var("MESSAGE_EXPIRY_MAX_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30 * 86400),
            prekey_low_threshold: std::env::var("PREKEY_LOW_THRESHOLD")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10),
            project_token_lifetime_secs: std::env::var("PROJECT_TOKEN_LIFETIME_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3600),
            projects_json: std::env::var("PROJECTS")
                .unwrap_or_else(|_| "[]".to_string()),
            relay_url: std::env::var("RELAY_URL").ok(),
            server_name: std::env::var("SERVER_NAME")
                .unwrap_or_else(|_| "Avalanche Server".to_string()),
            invite_domain: std::env::var("INVITE_DOMAIN")
                .unwrap_or_else(|_| "go.theavalanche.net".to_string()),
        }
    }
}
