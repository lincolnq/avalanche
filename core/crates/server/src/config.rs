//! Server configuration loaded from environment variables.
//!
//! All values have sensible defaults for local development. In production,
//! operators set environment variables to override them. The `DATABASE_URL`
//! default points at the Docker Compose Postgres instance.
//!
//! # Safety check: dev credentials + public bind = refuse to start
//!
//! The default `DATABASE_URL` embeds the dev password `actnet-dev`. If a
//! server reaches a non-loopback bind address while still using that
//! default, it almost certainly means an operator forgot to set
//! `DATABASE_URL` in a production environment. [`Config::from_env`] panics
//! with a clear message in that case so the process exits at startup
//! instead of running with dev secrets on a public interface.

/// Sentinel value identifying the local-dev DATABASE_URL. Used by the
/// safety check in [`Config::from_env`].
const DEFAULT_DEV_DATABASE_URL: &str = "postgres://actnet:actnet-dev@localhost/actnet";

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
        let config = Self::from_env_unchecked();
        config.assert_safe_to_start();
        config
    }

    fn from_env_unchecked() -> Self {
        Self {
            database_url: std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| DEFAULT_DEV_DATABASE_URL.to_string()),
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

    /// Panic with a clear message if the configuration looks like a
    /// production server still running with the dev DATABASE_URL. Called
    /// at process startup from [`Config::from_env`].
    ///
    /// Local dev needs to bind to `0.0.0.0` so iOS devices on the LAN can
    /// reach the server, *and* uses the dev DATABASE_URL — exactly the
    /// combination this check rejects. The documented dev recipes
    /// (`make dev`, `dev.py`) set `ACTNET_ALLOW_DEV_DB=1` to opt in. The
    /// opt-in is loud and intentional; production deploys never set it.
    fn assert_safe_to_start(&self) {
        let on_loopback = is_loopback_bind(&self.bind_addr);
        let is_dev_db = self.database_url == DEFAULT_DEV_DATABASE_URL;
        if !is_dev_db || on_loopback {
            return;
        }

        let allow_dev_db = std::env::var("ACTNET_ALLOW_DEV_DB").ok().as_deref() == Some("1");
        if allow_dev_db {
            tracing::warn!(
                bind_addr = %self.bind_addr,
                "running with dev DATABASE_URL on non-loopback bind \
                 (ACTNET_ALLOW_DEV_DB=1 set; intended for local dev only)"
            );
            return;
        }

        panic!(
            "\n\nrefusing to start: DATABASE_URL is the dev default \
             but BIND_ADDR='{}' is not loopback.\n\n\
             This combination almost always means a production deploy \
             forgot to set DATABASE_URL. Either:\n  \
             - set DATABASE_URL to your real database URL (production), or\n  \
             - set BIND_ADDR=127.0.0.1:<port> (loopback-only dev), or\n  \
             - set ACTNET_ALLOW_DEV_DB=1 (LAN-accessible dev with dev DB).\n",
            self.bind_addr,
        );
    }
}

/// True if the given `host:port` (or `[ipv6]:port`) binds to a loopback
/// address only — so the server can't be reached from the network. Used
/// to decide whether dev credentials are safe to leave in place.
fn is_loopback_bind(addr: &str) -> bool {
    let Some((host, _port)) = addr.rsplit_once(':') else {
        return false;
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if host == "localhost" {
        return true;
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(ip) => ip.is_loopback(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_detection() {
        assert!(is_loopback_bind("127.0.0.1:3000"));
        assert!(is_loopback_bind("localhost:3000"));
        assert!(is_loopback_bind("[::1]:3000"));
        assert!(!is_loopback_bind("0.0.0.0:3000"));
        assert!(!is_loopback_bind("192.168.1.5:3000"));
        assert!(!is_loopback_bind("[::]:3000"));
        assert!(!is_loopback_bind("garbage:3000"));
        assert!(!is_loopback_bind("no-colon"));
    }

    #[test]
    fn dev_db_with_loopback_is_allowed() {
        let mut c = Config::from_env_unchecked();
        c.database_url = DEFAULT_DEV_DATABASE_URL.to_string();
        c.bind_addr = "127.0.0.1:3000".to_string();
        c.assert_safe_to_start(); // should not panic
    }

    #[test]
    #[should_panic(expected = "refusing to start")]
    fn dev_db_with_public_bind_panics() {
        let mut c = Config::from_env_unchecked();
        c.database_url = DEFAULT_DEV_DATABASE_URL.to_string();
        c.bind_addr = "0.0.0.0:3000".to_string();
        c.assert_safe_to_start();
    }

    #[test]
    fn explicit_db_url_with_public_bind_is_allowed() {
        let mut c = Config::from_env_unchecked();
        c.database_url = "postgres://prod-user:s3cret@db.internal/avalanche".to_string();
        c.bind_addr = "0.0.0.0:3000".to_string();
        c.assert_safe_to_start(); // should not panic
    }
}
