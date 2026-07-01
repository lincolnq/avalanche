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

/// The hard-coded slug of the privileged superuser Project. Superuser
/// authority is membership in the Project with this slug (resolved via
/// `project_bots`) — not a pinned DID. The row is seeded empty at startup
/// (`db::projects::ensure_adminbot_project`); a bot becomes superuser by
/// registering with a bootstrap token that names this slug while the shared
/// secret is still active (docs/24). The admin API never mutates this
/// Project's membership, so superuser can't be granted over the wire.
pub const ADMINBOT_PROJECT_SLUG: &str = "adminbot";

/// Whether new accounts may register freely or only with a valid credential
/// (docs/24 closed registration). Defaults to [`Closed`] — fail safe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrationMode {
    /// Anyone may register (rate-limited by IP). A token, if present, is still
    /// validated/redeemed; a bootstrap token may still link the new account
    /// into a Project.
    Open,
    /// Registration is refused unless it presents a valid credential: a signed
    /// gatekeeper invite token, or — while no gatekeeper is installed — the
    /// configured shared secret. Fails closed.
    Closed,
}

impl RegistrationMode {
    fn from_env_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "open" => RegistrationMode::Open,
            _ => RegistrationMode::Closed,
        }
    }
}

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
    /// OAuth authorization-code lifetime in seconds (docs/25, same-device
    /// front-end). Short: the code is exchanged for a token immediately.
    /// Default: 2 minutes (a little slack over 60s for clock skew).
    pub oauth_auth_code_lifetime_secs: i64,
    /// OAuth device-code lifetime in seconds (docs/25, cross-device front-end).
    /// Long enough to scan a QR and consent on a phone. Default: 10 minutes.
    pub oauth_device_code_lifetime_secs: i64,
    /// Minimum interval in seconds a Project must wait between device-grant
    /// token polls (RFC 8628 `interval`/`slow_down`). Default: 5 seconds.
    pub oauth_device_poll_interval_secs: i64,
    /// Installed Projects as JSON array: [{"name":"...","url":"...","description":"..."}].
    pub projects_json: String,
    /// Push relay URL (e.g. "http://localhost:3002"). If unset, push is disabled.
    pub relay_url: Option<String>,
    /// Human-readable server name (shown to users during invite/onboarding).
    pub server_name: String,
    /// Domain used for deep link URLs in invite redirects (default: go.theavalanche.net).
    pub invite_domain: String,
    /// Operator's privacy policy URL shown to users during signup.
    /// Set via PRIVACY_POLICY_URL env var. Optional.
    pub privacy_policy_url: Option<String>,
    /// Whether registration is open to anyone or gated (docs/24). Set via
    /// `REGISTRATION_MODE=open|closed`; **default closed**.
    pub registration_mode: RegistrationMode,
    /// Bootstrap shared secret. While set — and only until a gatekeeper Project
    /// is installed — a registration presenting a matching `bootstrap_secret`
    /// is admitted, and may name a Project (incl. the superuser Project) to be
    /// linked into. This is the operator's setup-time root credential; it
    /// auto-disables the moment any Project is granted `registration.gatekeeper`.
    /// Set via `REGISTRATION_SHARED_SECRET`. Unset = no shared-secret path.
    pub registration_shared_secret: Option<String>,
    /// Directory for the local-filesystem attachment blob store (docs/35). The
    /// default `/var/lib/avalanche/attachments` works out of the box on the
    /// documented deploy: the `avalanche.service` unit runs as the `avalanche`
    /// user with `WorkingDirectory`/`ReadWritePaths=/var/lib/avalanche` (which
    /// `install.sh` creates), and the store auto-creates the `attachments`
    /// subdir. Other deployments take their cue from that path; the dev
    /// launchers (`make dev`, `dev.py`) override `ATTACHMENT_BLOB_DIR` to the
    /// repo-root `dev-state/attachments` tree.
    pub attachment_blob_dir: String,
    /// Attachment blob TTL in seconds (docs/35); the GC task deletes blobs past
    /// this. Default ~45 days — deliberately longer than message-queue
    /// retention. Set via `ATTACHMENT_BLOB_TTL_SECS`.
    pub attachment_blob_ttl_secs: i64,
    /// Per-attachment ciphertext size cap in bytes. Default 100 MB. Set via
    /// `ATTACHMENT_MAX_SIZE_BYTES`.
    pub attachment_max_size_bytes: i64,
    /// Rolling per-account upload quota in bytes per hour. Default 500 MB. Set
    /// via `ATTACHMENT_BYTES_PER_HOUR`.
    pub attachment_bytes_per_hour: i64,
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
            oauth_auth_code_lifetime_secs: std::env::var("OAUTH_AUTH_CODE_LIFETIME_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(120),
            oauth_device_code_lifetime_secs: std::env::var("OAUTH_DEVICE_CODE_LIFETIME_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(600),
            oauth_device_poll_interval_secs: std::env::var("OAUTH_DEVICE_POLL_INTERVAL_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5),
            projects_json: std::env::var("PROJECTS")
                .unwrap_or_else(|_| "[]".to_string()),
            relay_url: std::env::var("RELAY_URL").ok(),
            server_name: std::env::var("SERVER_NAME")
                .unwrap_or_else(|_| "Avalanche Server".to_string()),
            invite_domain: std::env::var("INVITE_DOMAIN")
                .unwrap_or_else(|_| "go.theavalanche.net".to_string()),
            registration_mode: std::env::var("REGISTRATION_MODE")
                .ok()
                .map(|s| RegistrationMode::from_env_str(&s))
                .unwrap_or(RegistrationMode::Closed),
            registration_shared_secret: std::env::var("REGISTRATION_SHARED_SECRET")
                .ok()
                .filter(|s| !s.is_empty()),
            privacy_policy_url: std::env::var("PRIVACY_POLICY_URL").ok().filter(|s| !s.is_empty()),
            attachment_blob_dir: std::env::var("ATTACHMENT_BLOB_DIR")
                .unwrap_or_else(|_| "/var/lib/avalanche/attachments".to_string()),
            attachment_blob_ttl_secs: std::env::var("ATTACHMENT_BLOB_TTL_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(45 * 86400),
            attachment_max_size_bytes: std::env::var("ATTACHMENT_MAX_SIZE_BYTES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(100 * 1024 * 1024),
            attachment_bytes_per_hour: std::env::var("ATTACHMENT_BYTES_PER_HOUR")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(500 * 1024 * 1024),
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

    #[test]
    fn registration_mode_parse() {
        assert_eq!(RegistrationMode::from_env_str("open"), RegistrationMode::Open);
        assert_eq!(RegistrationMode::from_env_str("OPEN"), RegistrationMode::Open);
        assert_eq!(RegistrationMode::from_env_str(" open "), RegistrationMode::Open);
        assert_eq!(RegistrationMode::from_env_str("closed"), RegistrationMode::Closed);
        // Anything unrecognized defaults to Closed — fail safe (closed is the
        // secure default; you must explicitly opt into open registration).
        assert_eq!(RegistrationMode::from_env_str("garbage"), RegistrationMode::Closed);
        assert_eq!(RegistrationMode::from_env_str(""), RegistrationMode::Closed);
    }
}
