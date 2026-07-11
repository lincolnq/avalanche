//! Push notification relay service.
//!
//! A standalone Axum service that mediates between homeservers and APNs/FCM.
//! Homeservers never see device push tokens; they send wakeup requests
//! addressed to opaque pseudonyms. The relay maps pseudonyms to device tokens
//! and fires content-free push notifications.
//!
//! # Privacy model
//!
//! - Clients register per-(user, server) pseudonyms, so a relay cannot link
//!   a user's activity across homeservers.
//! - Push payloads are content-free: Apple/Google only see that the app was
//!   pinged, not who sent a message or what it says.
//! - Pseudonyms rotate periodically (default weekly) with a grace period
//!   so old pseudonyms still work briefly.
//!
//! # Storage
//!
//! SQLite (encrypted via SQLCipher) using tokio-rusqlite.
//! Database path: `$DATA_DIR/relay.db` (defaults to `./relay.db`).
//!
//! # Dispatch by platform
//!
//! Each registration stores a `platform`; `wakeup` routes by it:
//!
//! - **apns** — the `a2` crate with token-based auth. The same `.p8` works for
//!   both sandbox and production endpoints; the relay builds one client per
//!   environment and routes each wakeup by the `environment` stored at
//!   registration time (clients pass it based on `#if DEBUG`). Configure via
//!   `APNS_KEY_PATH` (path to the .p8 private key), `APNS_KEY_ID`,
//!   `APNS_TEAM_ID`, and `APNS_BUNDLE_ID` (e.g. net.theavalanche.app). If
//!   `APNS_KEY_PATH` is unset the relay still runs but APNs wakeups are logged
//!   only. Payloads are content-free silent pushes (`content-available: 1`,
//!   PushType::Background, Priority::Normal).
//!
//! - **fcm** — FCM HTTP v1 with a service account. We mint an OAuth2 access
//!   token by signing a short-lived RS256 JWT (the legacy server-key API is
//!   decommissioned) and cache it. Messages are data-only + high priority so
//!   the Android `onMessageReceived` runs even when backgrounded; content-free
//!   like APNs. Configure via `FCM_SA_PATH` (path to the service-account JSON)
//!   and optionally `FCM_PROJECT_ID` (defaults to the JSON's `project_id`). If
//!   `FCM_SA_PATH` is unset, FCM wakeups are logged only.
//!
//! - **unifiedpush** — the `device_token` is the distributor endpoint URL
//!   (degoogled Android, routed through the relay so the homeserver stays out
//!   of the token business). We POST a content-free body to it. Because the
//!   URL is client-supplied and registration is unauthenticated, the POST is
//!   SSRF-guarded: https only, and the resolved host must be a global address
//!   (loopback/private/link-local/metadata ranges are rejected). No extra
//!   config; needs only outbound HTTPS.

use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use a2::{
    Client, ClientConfig, DefaultNotificationBuilder, Endpoint, NotificationBuilder,
    NotificationOptions, Priority, PushType,
};
use axum::{
    extract::{DefaultBodyLimit, Json, State},
    http::StatusCode,
    routing::post,
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::Mutex as AsyncMutex;
use tokio_rusqlite::Connection;
use tower_governor::{
    governor::GovernorConfigBuilder, key_extractor::SmartIpKeyExtractor, GovernorLayer,
};
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

// ── Types ───────────────────────────────────────────────────────────────────

struct RelayState {
    conn: Connection,
    apns: Option<Apns>,
    fcm: Option<Fcm>,
    /// Outbound client for UnifiedPush endpoint POSTs: redirects disabled (so a
    /// public URL can't 30x us into a private one) and a short timeout.
    up_http: reqwest::Client,
}

/// Two APNs clients (sandbox + production) sharing a single `.p8`. Wakeups
/// route to the right one based on the `environment` column from each
/// registration. Debug builds get sandbox device tokens; TestFlight/App
/// Store builds get production tokens — sending one to the wrong endpoint
/// returns `BadDeviceToken`.
struct Apns {
    sandbox: Client,
    production: Client,
    bundle_id: String,
}

impl Apns {
    /// Build both clients from env vars; returns None if APNS_KEY_PATH unset.
    fn from_env() -> Option<Self> {
        let key_path = std::env::var("APNS_KEY_PATH").ok()?;
        let key_id = std::env::var("APNS_KEY_ID")
            .expect("APNS_KEY_ID required when APNS_KEY_PATH is set");
        let team_id = std::env::var("APNS_TEAM_ID")
            .expect("APNS_TEAM_ID required when APNS_KEY_PATH is set");
        let bundle_id = std::env::var("APNS_BUNDLE_ID")
            .expect("APNS_BUNDLE_ID required when APNS_KEY_PATH is set");

        let mk = |endpoint: Endpoint| -> Client {
            let mut key = std::fs::File::open(&key_path)
                .unwrap_or_else(|e| panic!("failed to open {key_path}: {e}"));
            Client::token(&mut key, &key_id, &team_id, ClientConfig::new(endpoint))
                .expect("failed to build APNs client")
        };

        tracing::info!(bundle = %bundle_id, "APNs clients configured (sandbox + production)");
        Some(Self {
            sandbox: mk(Endpoint::Sandbox),
            production: mk(Endpoint::Production),
            bundle_id,
        })
    }

    async fn send_silent(&self, environment: &str, device_token: &str) -> Result<(), a2::Error> {
        let client = match environment {
            "production" => &self.production,
            _ => &self.sandbox, // default to sandbox for unknown/missing values
        };
        let payload = DefaultNotificationBuilder::new()
            .set_content_available()
            .build(
                device_token,
                NotificationOptions {
                    apns_topic: Some(&self.bundle_id),
                    apns_push_type: Some(PushType::Background),
                    apns_priority: Some(Priority::Normal),
                    ..Default::default()
                },
            );
        let resp = client.send(payload).await?;
        tracing::debug!(code = resp.code, apns_id = ?resp.apns_id, "APNs response");
        Ok(())
    }
}

// ── FCM (HTTP v1) ─────────────────────────────────────────────────────────

/// The subset of a Google service-account JSON we need to mint OAuth2 tokens.
#[derive(Deserialize, Clone)]
struct ServiceAccount {
    client_email: String,
    private_key: String,
    #[serde(default)]
    token_uri: String,
    #[serde(default)]
    project_id: String,
}

/// FCM HTTP v1 sender. Holds the service account, the target project, and a
/// cached OAuth2 access token (re-minted shortly before expiry).
struct Fcm {
    http: reqwest::Client,
    project_id: String,
    sa: ServiceAccount,
    token: AsyncMutex<Option<CachedToken>>,
}

struct CachedToken {
    value: String,
    /// Unix seconds at which we consider the token stale (real expiry minus a
    /// safety margin).
    expires_at: u64,
}

#[derive(Serialize)]
struct JwtClaims<'a> {
    iss: &'a str,
    scope: &'a str,
    aud: &'a str,
    iat: u64,
    exp: u64,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Fcm {
    /// Build from env; returns None if FCM_SA_PATH is unset.
    fn from_env() -> Option<Self> {
        let sa_path = std::env::var("FCM_SA_PATH").ok()?;
        let raw = std::fs::read_to_string(&sa_path)
            .unwrap_or_else(|e| panic!("failed to read {sa_path}: {e}"));
        let mut sa: ServiceAccount =
            serde_json::from_str(&raw).expect("failed to parse FCM service-account JSON");
        if sa.token_uri.is_empty() {
            sa.token_uri = "https://oauth2.googleapis.com/token".to_string();
        }
        let project_id = std::env::var("FCM_PROJECT_ID")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| (!sa.project_id.is_empty()).then(|| sa.project_id.clone()))
            .expect("FCM_PROJECT_ID required when the service-account JSON has no project_id");

        tracing::info!(%project_id, "FCM HTTP v1 configured");
        Some(Self {
            http: reqwest::Client::new(),
            project_id,
            sa,
            token: AsyncMutex::new(None),
        })
    }

    /// Return a valid OAuth2 access token, minting a fresh one if the cache is
    /// empty or within ~5 min of expiry.
    async fn access_token(&self) -> Result<String, String> {
        let mut guard = self.token.lock().await;
        let now = now_secs();
        if let Some(cached) = guard.as_ref() {
            if cached.expires_at > now {
                return Ok(cached.value.clone());
            }
        }
        let (value, expires_in) = self.mint_token().await?;
        // Refresh 5 min early to avoid races near the boundary.
        let expires_at = now + expires_in.saturating_sub(300);
        *guard = Some(CachedToken {
            value: value.clone(),
            expires_at,
        });
        Ok(value)
    }

    /// Sign a JWT with the service-account key and exchange it for an access
    /// token. Returns (token, expires_in_seconds).
    async fn mint_token(&self) -> Result<(String, u64), String> {
        let assertion = sign_sa_jwt(&self.sa, now_secs())?;
        #[derive(Deserialize)]
        struct TokenResp {
            access_token: String,
            expires_in: u64,
        }
        let resp = self
            .http
            .post(&self.sa.token_uri)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", &assertion),
            ])
            .send()
            .await
            .map_err(|e| format!("token request failed: {e}"))?;
        if !resp.status().is_success() {
            let code = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("token endpoint returned {code}: {body}"));
        }
        let tok: TokenResp = resp
            .json()
            .await
            .map_err(|e| format!("token response decode failed: {e}"))?;
        Ok((tok.access_token, tok.expires_in))
    }

    /// Send a content-free, high-priority data wakeup to one FCM token.
    async fn send_wakeup(&self, device_token: &str) -> Result<(), String> {
        let token = self.access_token().await?;
        let url = format!(
            "https://fcm.googleapis.com/v1/projects/{}/messages:send",
            self.project_id
        );
        let resp = self
            .http
            .post(&url)
            .bearer_auth(token)
            .json(&fcm_message(device_token))
            .send()
            .await
            .map_err(|e| format!("FCM send failed: {e}"))?;
        if !resp.status().is_success() {
            let code = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("FCM returned {code}: {body}"));
        }
        Ok(())
    }
}

/// Sign the OAuth2 assertion JWT for a service account. Split out for testing.
fn sign_sa_jwt(sa: &ServiceAccount, now: u64) -> Result<String, String> {
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
    let claims = JwtClaims {
        iss: &sa.client_email,
        scope: "https://www.googleapis.com/auth/firebase.messaging",
        aud: &sa.token_uri,
        iat: now,
        exp: now + 3600,
    };
    let key = EncodingKey::from_rsa_pem(sa.private_key.as_bytes())
        .map_err(|e| format!("invalid service-account private key: {e}"))?;
    encode(&Header::new(Algorithm::RS256), &claims, &key)
        .map_err(|e| format!("JWT signing failed: {e}"))
}

/// The FCM HTTP v1 message body for a content-free wakeup: data-only (no
/// `notification` key, so the app's handler always runs) and high priority.
fn fcm_message(device_token: &str) -> serde_json::Value {
    json!({
        "message": {
            "token": device_token,
            "android": { "priority": "high" },
            "data": { "w": "1" }
        }
    })
}

// ── UnifiedPush ─────────────────────────────────────────────────────────────

/// True if `ip` is one we must never POST to (loopback, private, link-local
/// incl. the cloud metadata 169.254.169.254, ULA, unspecified, etc.).
fn ip_is_blocked(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_blocked_v4(v4),
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || {
                    let s = v6.segments();
                    (s[0] & 0xfe00) == 0xfc00 // fc00::/7 unique-local
                        || (s[0] & 0xffc0) == 0xfe80 // fe80::/10 link-local
                }
                || v6.to_ipv4_mapped().is_some_and(is_blocked_v4)
        }
    }
}

fn is_blocked_v4(v4: Ipv4Addr) -> bool {
    v4.is_loopback()
        || v4.is_private()
        || v4.is_link_local()
        || v4.is_broadcast()
        || v4.is_documentation()
        || v4.is_unspecified()
        || v4.octets()[0] == 0 // 0.0.0.0/8
        || (v4.octets()[0] == 100 && (v4.octets()[1] & 0xc0) == 64) // 100.64/10 CGNAT
}

/// Validate a UnifiedPush endpoint URL: must be https with a host. Returns the
/// parsed URL. Host reachability (IP range) is checked at send time, after DNS
/// resolution, by `unifiedpush_blocked_host`.
fn validate_endpoint(url: &str) -> Result<reqwest::Url, String> {
    let parsed = reqwest::Url::parse(url).map_err(|e| format!("bad endpoint URL: {e}"))?;
    if parsed.scheme() != "https" {
        return Err(format!("endpoint scheme must be https, got {}", parsed.scheme()));
    }
    if parsed.host_str().is_none() {
        return Err("endpoint URL has no host".to_string());
    }
    Ok(parsed)
}

/// Resolve the URL's host and return true if any resolved address is blocked
/// (SSRF guard). An IP literal is checked directly; a hostname is resolved.
async fn unifiedpush_blocked_host(url: &reqwest::Url) -> bool {
    let host = match url.host_str() {
        Some(h) => h,
        None => return true,
    };
    if let Ok(ip) = host.parse::<IpAddr>() {
        return ip_is_blocked(ip);
    }
    let port = url.port_or_known_default().unwrap_or(443);
    match tokio::net::lookup_host((host, port)).await {
        Ok(addrs) => {
            let mut any = false;
            for a in addrs {
                any = true;
                if ip_is_blocked(a.ip()) {
                    return true;
                }
            }
            // No addresses resolved → treat as blocked (can't verify safety).
            !any
        }
        Err(_) => true,
    }
}

/// POST a content-free wakeup to a UnifiedPush distributor endpoint.
/// SSRF-guarded: https only and the resolved host must be global.
async fn send_unifiedpush(http: &reqwest::Client, endpoint: &str) -> Result<(), String> {
    let url = validate_endpoint(endpoint)?;
    if unifiedpush_blocked_host(&url).await {
        return Err("endpoint host resolves to a blocked address".to_string());
    }
    // A tiny body: distributors forward the bytes to the app, which only needs
    // the ping to wake and sync. UnifiedPush has no per-message TTL knob here.
    let resp = http
        .post(url)
        .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
        .header("TTL", "0")
        .body("w".to_string())
        .send()
        .await
        .map_err(|e| format!("UnifiedPush POST failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("UnifiedPush endpoint returned {}", resp.status()));
    }
    Ok(())
}

// ── Client endpoints ────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct RegisterRequest {
    /// Single pseudonym (legacy shape). Either this or `pseudonyms` (or both)
    /// must yield at least one pseudonym.
    #[serde(default)]
    pseudonym: Option<String>,
    /// Batch of pseudonyms sharing one device token. A device maps *many*
    /// pseudonyms (its account pseudonym + one per group, docs/03 §3.7) to the
    /// same token; registering them in one request keeps a launch-time
    /// re-register within the per-IP rate limit instead of firing N POSTs.
    #[serde(default)]
    pseudonyms: Vec<String>,
    device_token: String,
    platform: String,
    /// "sandbox" or "production" for APNs. Clients pick based on build
    /// flavor (`#if DEBUG` → sandbox). Ignored for non-APNs platforms.
    environment: String,
}

#[derive(Serialize)]
struct RegisterResponse {
    ok: bool,
}

/// Register or update pseudonym-to-device-token mappings. Accepts a single
/// `pseudonym` (legacy) and/or a `pseudonyms` batch; all map to the one token.
async fn register(
    State(state): State<Arc<RelayState>>,
    Json(req): Json<RegisterRequest>,
) -> Result<Json<RegisterResponse>, StatusCode> {
    // Combine the legacy single field with the batch, de-duplicating.
    let mut all = req.pseudonyms;
    if let Some(p) = req.pseudonym {
        all.push(p);
    }
    all.sort();
    all.dedup();
    if all.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let count = all.len();
    let device_token = req.device_token;
    let platform = req.platform;
    let environment = req.environment;
    let result = state.conn.call(move |conn| {
        let tx = conn.transaction()?;
        for pseudonym in &all {
            tx.execute(
                "INSERT OR REPLACE INTO push_registrations \
                 (pseudonym, device_token, platform, environment, registered_at, rotated_at) \
                 VALUES (?1, ?2, ?3, ?4, strftime('%s','now'), NULL)",
                rusqlite::params![pseudonym, device_token, platform, environment],
            )?;
        }
        tx.commit()?;
        Ok(())
    }).await;

    match result {
        Ok(()) => {
            tracing::info!(count, "registered pseudonym(s)");
            Ok(Json(RegisterResponse { ok: true }))
        }
        Err(e) => {
            tracing::error!(error = %e, "failed to register pseudonym(s)");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[derive(Deserialize)]
struct UnregisterRequest {
    pseudonym: String,
}

/// Mark a pseudonym as rotated (grace period: kept for 7 days).
async fn unregister(
    State(state): State<Arc<RelayState>>,
    Json(req): Json<UnregisterRequest>,
) -> StatusCode {
    let result = state.conn.call(move |conn| {
        let rows = conn.execute(
            "UPDATE push_registrations SET rotated_at = strftime('%s','now') \
             WHERE pseudonym = ?1 AND rotated_at IS NULL",
            rusqlite::params![req.pseudonym],
        )?;
        Ok(rows)
    }).await;

    match result {
        Ok(1..) => {
            tracing::info!("marked pseudonym as rotated (7-day grace period)");
            StatusCode::OK
        }
        Ok(_) => StatusCode::NOT_FOUND,
        Err(e) => {
            tracing::error!(error = %e, "failed to unregister pseudonym");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

// ── Homeserver endpoints ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct WakeupRequest {
    pseudonyms: Vec<String>,
}

#[derive(Serialize)]
struct WakeupResponse {
    woken: Vec<String>,
    unknown: Vec<String>,
}

/// Send content-free push wakeups to one or more pseudonyms.
/// Pseudonyms within the 7-day grace period after rotation are still honoured.
async fn wakeup(
    State(state): State<Arc<RelayState>>,
    Json(req): Json<WakeupRequest>,
) -> Json<WakeupResponse> {
    let mut woken = Vec::new();
    let mut unknown = Vec::new();

    for pseudonym in req.pseudonyms {
        let ps = pseudonym.clone();
        let result = state.conn.call(move |conn| {
            use rusqlite::OptionalExtension as _;
            conn.query_row(
                "SELECT device_token, platform, environment FROM push_registrations \
                 WHERE pseudonym = ?1 \
                 AND (rotated_at IS NULL OR rotated_at > strftime('%s','now') - 604800)",
                rusqlite::params![ps],
                |row| Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                )),
            ).optional()
            .map_err(Into::into)
        }).await;

        match result {
            Ok(Some((device_token, platform, environment))) => {
                let token_prefix = &device_token[..8.min(device_token.len())];
                match platform.as_str() {
                    "apns" => {
                        if let Some(apns) = &state.apns {
                            match apns.send_silent(&environment, &device_token).await {
                                Ok(()) => {
                                    tracing::info!(token_prefix, %environment, "sent APNs wakeup");
                                    woken.push(pseudonym);
                                }
                                Err(e) => {
                                    tracing::error!(token_prefix, %environment, error = %e, "APNs send failed");
                                    unknown.push(pseudonym);
                                }
                            }
                        } else {
                            tracing::info!(token_prefix, "APNs not configured, logging wakeup only");
                            woken.push(pseudonym);
                        }
                    }
                    "fcm" => {
                        if let Some(fcm) = &state.fcm {
                            match fcm.send_wakeup(&device_token).await {
                                Ok(()) => {
                                    tracing::info!(token_prefix, "sent FCM wakeup");
                                    woken.push(pseudonym);
                                }
                                Err(e) => {
                                    tracing::error!(token_prefix, error = %e, "FCM send failed");
                                    unknown.push(pseudonym);
                                }
                            }
                        } else {
                            tracing::info!(token_prefix, "FCM not configured, logging wakeup only");
                            woken.push(pseudonym);
                        }
                    }
                    "unifiedpush" => {
                        match send_unifiedpush(&state.up_http, &device_token).await {
                            Ok(()) => {
                                tracing::info!(token_prefix, "sent UnifiedPush wakeup");
                                woken.push(pseudonym);
                            }
                            Err(e) => {
                                tracing::error!(token_prefix, error = %e, "UnifiedPush send failed");
                                unknown.push(pseudonym);
                            }
                        }
                    }
                    _ => {
                        tracing::warn!(%platform, token_prefix, "unsupported platform, skipping");
                        unknown.push(pseudonym);
                    }
                }
            }
            Ok(None) => {
                tracing::debug!(pseudonym = %pseudonym, "unknown or expired pseudonym, skipping");
                unknown.push(pseudonym);
            }
            Err(e) => {
                tracing::error!(error = %e, "db error during wakeup lookup");
                unknown.push(pseudonym);
            }
        }
    }

    Json(WakeupResponse { woken, unknown })
}

// ── Maintenance ─────────────────────────────────────────────────────────────

/// Periodically delete pseudonyms whose 7-day grace period has elapsed.
async fn gc_loop(conn: Connection) {
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(3600));
    loop {
        interval.tick().await;
        let result = conn.call(|conn| {
            let n = conn.execute(
                "DELETE FROM push_registrations \
                 WHERE rotated_at IS NOT NULL AND rotated_at < strftime('%s','now') - 604800",
                [],
            )?;
            Ok(n)
        }).await;
        match result {
            Ok(n) if n > 0 => tracing::info!(deleted = n, "GC: removed expired pseudonyms"),
            Ok(_) => {}
            Err(e) => tracing::error!(error = %e, "GC: failed to delete expired pseudonyms"),
        }
    }
}

// ── Main ────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let bind_addr = std::env::var("RELAY_BIND_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:3002".to_string());

    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| ".".to_string());
    let db_path = format!("{}/relay.db", data_dir);

    let conn = Connection::open(&db_path)
        .await
        .expect("failed to open relay database");

    conn.call(|conn| {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS push_registrations (
                 pseudonym      TEXT PRIMARY KEY,
                 device_token   TEXT NOT NULL,
                 platform       TEXT NOT NULL,
                 environment    TEXT NOT NULL DEFAULT 'sandbox',
                 registered_at  INTEGER NOT NULL DEFAULT (strftime('%s','now')),
                 rotated_at     INTEGER
             );",
        )?;
        // Migration for existing pre-environment databases. SQLite has no
        // IF NOT EXISTS on ADD COLUMN, so just swallow "duplicate column".
        let _ = conn.execute(
            "ALTER TABLE push_registrations ADD COLUMN environment TEXT NOT NULL DEFAULT 'sandbox'",
            [],
        );
        Ok(())
    })
    .await
    .expect("failed to create schema");

    let gc_conn = Connection::open(&db_path)
        .await
        .expect("failed to open gc connection");
    tokio::spawn(gc_loop(gc_conn));

    let apns = Apns::from_env();
    if apns.is_none() {
        tracing::warn!("APNS_KEY_PATH not set — APNs wakeups will be logged but not delivered");
    }

    let fcm = Fcm::from_env();
    if fcm.is_none() {
        tracing::warn!("FCM_SA_PATH not set — FCM wakeups will be logged but not delivered");
    }

    // UnifiedPush endpoints are client-supplied URLs, so disable redirects (a
    // public endpoint must not be able to bounce us into a private address) and
    // keep the timeout short.
    let up_http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .expect("failed to build UnifiedPush HTTP client");

    let state = Arc::new(RelayState {
        conn,
        apns,
        fcm,
        up_http,
    });

    // Per-IP rate limits. SmartIpKeyExtractor reads X-Forwarded-For when
    // present (set automatically by Caddy in the production deployment) and
    // falls back to the connection peer IP. The production relay binds to
    // 127.0.0.1, so only Caddy can reach it — direct connections that could
    // spoof X-Forwarded-For aren't possible from outside the droplet.
    //
    // - register/unregister: one client touches these on app launch and on
    //   weekly rotation. 10/min/IP with burst 5 covers NATed networks while
    //   capping a flood attacker at ~600 inserts/hour.
    // - wakeup: per-IP is per-homeserver in the homeserver→relay path.
    //   60/min sustained, burst 30 handles announcement-group fan-out.
    let register_limit = Arc::new(
        GovernorConfigBuilder::default()
            .per_second(6)
            .burst_size(5)
            .key_extractor(SmartIpKeyExtractor)
            .finish()
            .expect("register rate limiter"),
    );
    let wakeup_limit = Arc::new(
        GovernorConfigBuilder::default()
            .per_second(1)
            .burst_size(30)
            .key_extractor(SmartIpKeyExtractor)
            .finish()
            .expect("wakeup rate limiter"),
    );

    let register_routes = Router::new()
        .route("/v1/register", post(register))
        .route("/v1/unregister", post(unregister))
        .layer(GovernorLayer { config: register_limit });

    let wakeup_routes = Router::new()
        .route("/v1/wakeup", post(wakeup))
        .layer(GovernorLayer { config: wakeup_limit });

    let app = register_routes
        .merge(wakeup_routes)
        // Cap request bodies at 4 KB. Even a wakeup with many pseudonyms
        // (each ~44 chars b64) fits well under this; a register payload
        // (token ~64 hex + a few short strings) is well under 1 KB.
        .layer(DefaultBodyLimit::max(4096))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    tracing::info!(bind = %bind_addr, "starting push relay");

    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .expect("failed to bind");

    axum::serve(listener, app).await.expect("relay error");
}

#[cfg(test)]
mod tests {
    use super::*;

    // A throwaway 2048-bit RSA key generated solely for these tests — it signs
    // nothing real and guards no secret.
    const TEST_RSA_PEM: &str = "-----BEGIN PRIVATE KEY-----\n\
MIIEvAIBADANBgkqhkiG9w0BAQEFAASCBKYwggSiAgEAAoIBAQC4gXTUj57tzhBD\n\
CIGuyEJL3HULkqKMymx4yYIlCkVbbushgd+yDNA5dWF7KYPFN/9uiWOPm/NFM1C1\n\
mqGuOZeBC8ynlLmFWR3H+0Qn6HFRCX60kh20tMqHznR8mVAGq6n3+xgBKkpnVOa3\n\
qVgqBt68LMED4mPN98YxFx4+IK84Xb8LLRWo6fQjbE+hEp6K6PKWD/nIsZladKDy\n\
jZzo+PsJJsvCiIDz4HkTbdF5nmMw0suAgLyTsM5wKBYB7ir+B49OZiiJQyvW3+ys\n\
Me3k/Sp1q0lKMexyOOxoY2BRyNy6ib4p0S87f/Lo5Au2kuQmE3iaAxwgnjiQWXo8\n\
PkpR0oyLAgMBAAECggEACrX1LsvBmaN/PSz2vktA0f+Tyd6y0gKERoTIKddNTHAF\n\
dVYBQuhMhDFkvc4cqKvGB8gw/+q5NhizBD/cR+1u9p5VPJs9I9kXCf9zpu9u+JnZ\n\
AamD0PQ316dsCrmptCFuBgfNDnBCjnoWxK2vgQ0SDBF7CPsw+Ql7t2jUqB2knlx1\n\
UbMFFWEWEOTubX6Xv/oLksADBBRVPZIyAdb8pTDCsJXt2cfyDg6FQr3Vh/vWGGKY\n\
ygEHg2gfBeDH8Z5FlmidVa1oV3DC1xBRJgIbNjRjY2FUomEA8kSu4pQ5VeuO1/TJ\n\
JSv+wBnFlD6tP1hEL7U3fO63M6Dtt/d+3EZdkw08JQKBgQD8PiObtv/Ai24iLWTL\n\
4FoWEVzpRFxmDgl0KTZyuWDnBWQn1nvtTnRTPP0mRr+mt5nvRerPI37gji2uY5DG\n\
itDOc5Kxau5g2SsaW3MT0ac5NJpAJF+JzDT6uRYhs6h3n2KowFU28wbAXOqQMNOV\n\
5Nc0JP6ikcjLZQogsB4lxNJ9jQKBgQC7QQUeAHATf5E3JXz0cMVUCXRVYXzHPT/V\n\
8rCXr7fYtcPBPu3NnYLNgOjlNr+CSZo3IQ/R40iLAIG8AWY6HQDfUSMRDTukRzVt\n\
/+ojKfAg18dR6Aqd6VzbF32yv5OCG0dO4SXYcvb7TTXoh1x9dajs8CRk8cXknR1M\n\
cQbRlt7wdwKBgEg1Xaow6+vpvkBocEdw1TkmBUv9ttm8QPaQ6bZT3SqlP3OsEdPw\n\
NpqxheKUND78pkN552sexS5xZSBb/lDn4jiHm0HQ06bD5HPqI/pTdSSKK3IbN4JS\n\
BASWQYCqVprP9MVMMdjGH3On5bobUCbD2Ntjj8VoKGwZY5nR8P9s5bjtAoGAWOKr\n\
UCt7B/Zk8x1rbAjf0l1OiHznIxhS5fb2lnpFtavKST/a1+Btx6jqZGuRioHvnz2l\n\
accOl1TdQGYVpX5A/MBh+eUjK37VwOpatOhiYOSsa5fO+lhcyG8lLqU7muXh+nJr\n\
aYxg6rci4MboQ3GHhzkSvYv6mONvleqNBw4rs08CgYBVfEhvL1Z+i9oqUQfau2qC\n\
6MSemRugMGIjwKXsApbtW8RwS9Vh0cwtppeCzaC6bsfkuGjLE+YJ2lyDlTOwjES9\n\
bQTA+sehNXyksmr21+YaxYAjwLSdJs45hb1fImWHGZf8k8cXVSh7IUgjcDjN1VOW\n\
hVtiGBYhl1ucCLq/CaUowQ==\n\
-----END PRIVATE KEY-----";

    fn test_sa() -> ServiceAccount {
        ServiceAccount {
            client_email: "relay@example.iam.gserviceaccount.com".to_string(),
            private_key: TEST_RSA_PEM.to_string(),
            token_uri: "https://oauth2.googleapis.com/token".to_string(),
            project_id: "test-project".to_string(),
        }
    }

    // ── SSRF: IP classification ──────────────────────────────────────────────

    #[test]
    fn blocks_private_and_special_v4() {
        for s in [
            "127.0.0.1",       // loopback
            "10.1.2.3",        // private
            "172.16.5.4",      // private
            "192.168.0.1",     // private
            "169.254.169.254", // link-local / cloud metadata
            "0.0.0.0",         // unspecified / 0.0.0.0/8
            "100.64.0.1",      // CGNAT
            "255.255.255.255", // broadcast
        ] {
            let ip: IpAddr = s.parse().unwrap();
            assert!(ip_is_blocked(ip), "{s} should be blocked");
        }
    }

    #[test]
    fn allows_public_v4() {
        for s in ["8.8.8.8", "1.1.1.1", "93.184.216.34"] {
            let ip: IpAddr = s.parse().unwrap();
            assert!(!ip_is_blocked(ip), "{s} should be allowed");
        }
    }

    #[test]
    fn blocks_special_v6() {
        for s in ["::1", "fe80::1", "fc00::1", "::", "::ffff:127.0.0.1", "::ffff:10.0.0.1"] {
            let ip: IpAddr = s.parse().unwrap();
            assert!(ip_is_blocked(ip), "{s} should be blocked");
        }
        // A global v6 address is allowed.
        let ok: IpAddr = "2606:4700:4700::1111".parse().unwrap();
        assert!(!ip_is_blocked(ok));
    }

    // ── UnifiedPush endpoint validation ──────────────────────────────────────

    #[test]
    fn validate_endpoint_requires_https() {
        assert!(validate_endpoint("http://push.example.com/UP?token=abc").is_err());
        assert!(validate_endpoint("ftp://push.example.com/x").is_err());
        assert!(validate_endpoint("not a url").is_err());
        assert!(validate_endpoint("https://push.example.com/UP?token=abc").is_ok());
    }

    #[tokio::test]
    async fn unifiedpush_host_guard_blocks_ip_literals() {
        for u in [
            "https://127.0.0.1/UP",
            "https://10.0.0.5/UP",
            "https://169.254.169.254/latest/meta-data",
            "https://[::1]/UP",
        ] {
            let url = reqwest::Url::parse(u).unwrap();
            assert!(unifiedpush_blocked_host(&url).await, "{u} host should be blocked");
        }
        // Public IP literal passes the host guard.
        let ok = reqwest::Url::parse("https://8.8.8.8/UP").unwrap();
        assert!(!unifiedpush_blocked_host(&ok).await);
    }

    #[tokio::test]
    async fn send_unifiedpush_rejects_unsafe_targets() {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        // http scheme rejected before any network use.
        assert!(send_unifiedpush(&http, "http://push.example.com/UP").await.is_err());
        // loopback rejected by the host guard.
        assert!(send_unifiedpush(&http, "https://127.0.0.1/UP").await.is_err());
    }

    // ── FCM payload + JWT ────────────────────────────────────────────────────

    #[test]
    fn fcm_message_is_data_only_high_priority() {
        let msg = fcm_message("device-token-123");
        let m = &msg["message"];
        assert_eq!(m["token"], "device-token-123");
        assert_eq!(m["android"]["priority"], "high");
        assert_eq!(m["data"]["w"], "1");
        // Must NOT carry a `notification` block, or Android won't run our handler
        // when backgrounded.
        assert!(m.get("notification").is_none());
    }

    #[test]
    fn sa_jwt_has_expected_claims() {
        use base64::Engine as _;
        let sa = test_sa();
        let now = 1_700_000_000;
        let jwt = sign_sa_jwt(&sa, now).expect("signing should succeed");

        let parts: Vec<&str> = jwt.split('.').collect();
        assert_eq!(parts.len(), 3, "JWT must have header.payload.signature");
        assert!(!parts[2].is_empty(), "signature must be present");

        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(parts[1])
            .expect("payload should be base64url");
        let claims: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(claims["iss"], sa.client_email);
        assert_eq!(claims["aud"], sa.token_uri);
        assert_eq!(claims["scope"], "https://www.googleapis.com/auth/firebase.messaging");
        assert_eq!(claims["iat"], now);
        assert_eq!(claims["exp"], now + 3600);
    }

    #[test]
    fn service_account_parses_and_defaults_token_uri() {
        // project_id + token_uri present.
        let full = r#"{"client_email":"a@b.iam.gserviceaccount.com","private_key":"x","token_uri":"https://t/","project_id":"p"}"#;
        let sa: ServiceAccount = serde_json::from_str(full).unwrap();
        assert_eq!(sa.project_id, "p");
        assert_eq!(sa.token_uri, "https://t/");
        // Minimal JSON: missing token_uri/project_id default to empty (the
        // builder fills token_uri and requires project_id from env).
        let min = r#"{"client_email":"a@b","private_key":"x"}"#;
        let sa: ServiceAccount = serde_json::from_str(min).unwrap();
        assert!(sa.token_uri.is_empty());
        assert!(sa.project_id.is_empty());
    }
}
