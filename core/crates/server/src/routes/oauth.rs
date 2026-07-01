//! OAuth 2.0 endpoints for Project login ("Sign in with Avalanche").
//!
//! See `docs/25-project-login.md`. Two front-ends share this back-end:
//!
//! - **Same-device — Authorization Code + PKCE** (RFC 6749 / 7636):
//!   `POST /v1/oauth/authorize-code` (session-auth) mints a code post-consent;
//!   `POST /v1/oauth/token` (grant_type=authorization_code) exchanges it.
//! - **Cross-device — Device Authorization Grant** (RFC 8628):
//!   `POST /v1/oauth/device_authorization` starts it; the phone approves via
//!   `POST /v1/oauth/device/approve` (session-auth); the Project polls
//!   `POST /v1/oauth/token` (grant_type=device_code).
//!
//! The minted `access_token` is a `project_tokens` row, so the existing
//! `GET /v1/project-token/verify` resolves the DID unchanged.

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Form, Json, Router,
};
use base64::prelude::*;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::{
    db,
    error::ServerError,
    middleware::{auth::AuthDevice, client_ip::ClientIp, rate_limit},
    state::AppState,
};

const DEVICE_CODE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/oauth/authorize-code", post(authorize_code))
        .route("/v1/oauth/device_authorization", post(device_authorization))
        .route("/v1/oauth/device/approve", post(device_approve))
        .route("/v1/oauth/token", post(token))
}

// ── OAuth client registry (parsed from the PROJECTS config) ─────────────────

/// A Project's OAuth-client registration, embedded in its `PROJECTS` entry.
/// Login fields are optional — a Project that does not do login omits them.
#[derive(Debug, Deserialize)]
struct ClientEntry {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    client_id: Option<String>,
    #[serde(default)]
    redirect_uris: Vec<String>,
    #[serde(default)]
    official: bool,
}

struct ResolvedClient {
    project_url: String,
    redirect_uris: Vec<String>,
    #[allow(dead_code)]
    name: Option<String>,
    #[allow(dead_code)]
    official: bool,
}

/// Find a registered OAuth client by `client_id` in the `PROJECTS` config.
/// A registered client must carry both a `client_id` and a `url` (the token
/// audience); entries missing either are not valid login clients.
fn find_client(projects_json: &str, client_id: &str) -> Option<ResolvedClient> {
    let entries: Vec<ClientEntry> = serde_json::from_str(projects_json).ok()?;
    entries.into_iter().find_map(|e| {
        let cid = e.client_id?;
        let url = e.url?;
        if cid == client_id {
            Some(ResolvedClient {
                project_url: url,
                redirect_uris: e.redirect_uris,
                name: e.name,
                official: e.official,
            })
        } else {
            None
        }
    })
}

// ── PKCE (RFC 7636) ─────────────────────────────────────────────────────────

/// Verify a PKCE `code_verifier` against a stored `code_challenge`. Only S256
/// is supported: `challenge == base64url_nopad(SHA-256(verifier))`.
fn verify_pkce(verifier: &str, challenge: &str, method: &str) -> bool {
    use sha2::{Digest, Sha256};
    if method != "S256" {
        return false;
    }
    let digest = Sha256::digest(verifier.as_bytes());
    let computed = BASE64_URL_SAFE_NO_PAD.encode(digest);
    // Length-checked equality; both are fixed-length base64url of a 32-byte hash.
    computed.as_bytes().len() == challenge.as_bytes().len()
        && computed
            .as_bytes()
            .iter()
            .zip(challenge.as_bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

// ── Token / code generation ─────────────────────────────────────────────────

fn random_token() -> String {
    use rand::Rng;
    let bytes: [u8; 32] = rand::rng().random();
    BASE64_URL_SAFE_NO_PAD.encode(bytes)
}

/// Generate a short, human-typeable device user code, formatted `XXXX-XXXX`.
/// Alphabet excludes vowels and ambiguous characters (0/O, 1/I/L).
fn random_user_code() -> String {
    use rand::Rng;
    const ALPHABET: &[u8] = b"BCDFGHJKMNPQRSTVWXZ23456789";
    let mut rng = rand::rng();
    let mut s = String::with_capacity(9);
    for i in 0..8 {
        if i == 4 {
            s.push('-');
        }
        let idx = rng.random_range(0..ALPHABET.len());
        s.push(ALPHABET[idx] as char);
    }
    s
}

// ── OAuth error responses (RFC-shaped JSON) ─────────────────────────────────

/// OAuth error codes returned by the client-facing endpoints as
/// `{ "error": "<code>" }` with the appropriate HTTP status (RFC 6749 §5.2,
/// RFC 8628 §3.5).
enum OAuthError {
    InvalidRequest,
    InvalidClient,
    InvalidGrant,
    UnsupportedGrantType,
    AuthorizationPending,
    SlowDown,
    ExpiredToken,
    AccessDenied,
    Server,
}

impl OAuthError {
    fn code(&self) -> &'static str {
        match self {
            OAuthError::InvalidRequest => "invalid_request",
            OAuthError::InvalidClient => "invalid_client",
            OAuthError::InvalidGrant => "invalid_grant",
            OAuthError::UnsupportedGrantType => "unsupported_grant_type",
            OAuthError::AuthorizationPending => "authorization_pending",
            OAuthError::SlowDown => "slow_down",
            OAuthError::ExpiredToken => "expired_token",
            OAuthError::AccessDenied => "access_denied",
            OAuthError::Server => "server_error",
        }
    }

    fn status(&self) -> StatusCode {
        match self {
            OAuthError::InvalidClient => StatusCode::UNAUTHORIZED,
            OAuthError::Server => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::BAD_REQUEST,
        }
    }
}

impl IntoResponse for OAuthError {
    fn into_response(self) -> Response {
        (self.status(), Json(serde_json::json!({ "error": self.code() }))).into_response()
    }
}

impl From<sqlx::Error> for OAuthError {
    fn from(e: sqlx::Error) -> Self {
        tracing::error!("oauth db error: {e}");
        OAuthError::Server
    }
}

// ── POST /v1/oauth/authorize-code (session-auth) ────────────────────────────

#[derive(Deserialize)]
struct AuthorizeCodeRequest {
    client_id: String,
    redirect_uri: String,
    code_challenge: String,
    #[serde(default = "default_challenge_method")]
    code_challenge_method: String,
    #[serde(default)]
    scope: Option<String>,
}

fn default_challenge_method() -> String {
    "S256".to_string()
}

#[derive(Serialize)]
struct AuthorizeCodeResponse {
    code: String,
}

/// The app calls this after the user consents (same-device front-end). It mints
/// a single-use authorization code bound to the account, the PKCE challenge, and
/// the exact `redirect_uri`. The app then redirects the browser to
/// `redirect_uri?code=...&state=...`.
async fn authorize_code(
    State(state): State<AppState>,
    auth: AuthDevice,
    Json(req): Json<AuthorizeCodeRequest>,
) -> Result<Json<AuthorizeCodeResponse>, ServerError> {
    if req.code_challenge_method != "S256" {
        return Err(ServerError::BadRequest(
            "only the S256 PKCE method is supported".into(),
        ));
    }

    let client = find_client(&state.config.projects_json, &req.client_id)
        .ok_or_else(|| ServerError::BadRequest("unknown client_id".into()))?;
    if !client.redirect_uris.iter().any(|u| u == &req.redirect_uri) {
        return Err(ServerError::BadRequest(
            "redirect_uri not registered for this client".into(),
        ));
    }

    let mut conn = state.db.acquire().await?;
    let device = db::devices::find_by_pk(&mut conn, auth.device_pk)
        .await?
        .ok_or(ServerError::Internal("authenticated device not found".into()))?;

    let code = random_token();
    db::oauth_grants::create_auth_code(
        &mut conn,
        &code,
        device.account_id,
        &req.client_id,
        &client.project_url,
        &req.redirect_uri,
        &req.code_challenge,
        &req.code_challenge_method,
        req.scope.as_deref(),
        state.config.oauth_auth_code_lifetime_secs,
    )
    .await?;

    Ok(Json(AuthorizeCodeResponse { code }))
}

// ── POST /v1/oauth/device_authorization (unauthenticated, client) ───────────

#[derive(Deserialize)]
struct DeviceAuthRequest {
    client_id: String,
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Serialize)]
struct DeviceAuthResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    verification_uri_complete: String,
    expires_in: i64,
    interval: i64,
}

/// Start a cross-device (device-grant) login. Returns a `device_code` the
/// Project polls and a `user_code`/QR the user scans on their phone.
async fn device_authorization(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Form(req): Form<DeviceAuthRequest>,
) -> Result<Json<DeviceAuthResponse>, OAuthError> {
    let mut conn = state.db.acquire().await?;

    if !db::ip_rate_limits::check_and_increment(
        &mut conn,
        &ip,
        rate_limit::ACTION_OAUTH_DEVICE_AUTH,
        rate_limit::LIMIT_OAUTH_DEVICE_AUTH,
        rate_limit::WINDOW_OAUTH_DEVICE_AUTH,
    )
    .await?
    {
        return Err(OAuthError::InvalidRequest);
    }

    let client = find_client(&state.config.projects_json, &req.client_id)
        .ok_or(OAuthError::InvalidClient)?;

    let device_code = random_token();
    let user_code = random_user_code();

    db::oauth_grants::create_device(
        &mut conn,
        &device_code,
        &user_code,
        &req.client_id,
        &client.project_url,
        req.scope.as_deref(),
        state.config.oauth_device_code_lifetime_secs,
    )
    .await?;

    // The verification URI is the `authorize` Universal Link that opens the app
    // (the app is the authorization endpoint; there is no server login page).
    let base = format!("https://{}/authorize", state.config.invite_domain);
    let verification_uri_complete = format!(
        "{base}?user_code={}&server_url={}&client_id={}",
        urlencode(&user_code),
        urlencode(&state.config.server_url),
        urlencode(&req.client_id),
    );

    Ok(Json(DeviceAuthResponse {
        device_code,
        user_code,
        verification_uri: base,
        verification_uri_complete,
        expires_in: state.config.oauth_device_code_lifetime_secs,
        interval: state.config.oauth_device_poll_interval_secs,
    }))
}

/// Minimal percent-encoding for the query-parameter values we emit (all are
/// server-generated: a user code, a URL, and a client id).
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// ── POST /v1/oauth/device/approve (session-auth) ────────────────────────────

#[derive(Deserialize)]
struct DeviceApproveRequest {
    user_code: String,
    client_id: String,
}

#[derive(Serialize)]
struct DeviceApproveResponse {
    project_url: String,
}

/// The phone calls this after the user consents (cross-device front-end). It
/// binds the authenticated account to the pending device grant and mints the
/// access token, which the polling Project then collects.
async fn device_approve(
    State(state): State<AppState>,
    auth: AuthDevice,
    Json(req): Json<DeviceApproveRequest>,
) -> Result<Json<DeviceApproveResponse>, ServerError> {
    let mut conn = state.db.acquire().await?;

    // Normalize the user code (users may enter it lowercased / without dash).
    let normalized = req.user_code.trim().to_ascii_uppercase();

    let grant = db::oauth_grants::find_pending_device_by_user_code(&mut conn, &normalized)
        .await?
        .ok_or(ServerError::NotFound)?;

    if grant.client_id != req.client_id {
        return Err(ServerError::BadRequest("client_id does not match".into()));
    }

    let device = db::devices::find_by_pk(&mut conn, auth.device_pk)
        .await?
        .ok_or(ServerError::Internal("authenticated device not found".into()))?;

    // Mint the access token (a project token) for this account + audience.
    let access_token = random_token();
    db::project_tokens::create(
        &mut conn,
        &access_token,
        device.account_id,
        &grant.project_url,
        state.config.project_token_lifetime_secs,
    )
    .await?;

    let updated =
        db::oauth_grants::approve_device(&mut conn, &grant.code, device.account_id, &access_token)
            .await?;
    if updated == 0 {
        // Raced (expired-swept or approved concurrently) between lookup and update.
        return Err(ServerError::Conflict("device grant no longer pending".into()));
    }

    Ok(Json(DeviceApproveResponse {
        project_url: grant.project_url,
    }))
}

// ── POST /v1/oauth/token (unauthenticated, client) ──────────────────────────

#[derive(Deserialize)]
struct TokenRequest {
    grant_type: String,
    // authorization_code flow:
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    redirect_uri: Option<String>,
    #[serde(default)]
    code_verifier: Option<String>,
    // device_code flow:
    #[serde(default)]
    device_code: Option<String>,
    // both:
    #[serde(default)]
    client_id: Option<String>,
}

#[derive(Serialize)]
struct TokenResponse {
    access_token: String,
    token_type: &'static str,
    expires_in: i64,
    auth_time: Option<i64>,
}

async fn token(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Form(req): Form<TokenRequest>,
) -> Result<Json<TokenResponse>, OAuthError> {
    let mut conn = state.db.acquire().await?;

    if !db::ip_rate_limits::check_and_increment(
        &mut conn,
        &ip,
        rate_limit::ACTION_OAUTH_TOKEN,
        rate_limit::LIMIT_OAUTH_TOKEN,
        rate_limit::WINDOW_OAUTH_TOKEN,
    )
    .await?
    {
        return Err(OAuthError::InvalidRequest);
    }

    match req.grant_type.as_str() {
        "authorization_code" => token_authorization_code(&state, &mut conn, req).await,
        g if g == DEVICE_CODE_GRANT => token_device_code(&state, &mut conn, req).await,
        _ => Err(OAuthError::UnsupportedGrantType),
    }
}

async fn token_authorization_code(
    state: &AppState,
    conn: &mut sqlx::PgConnection,
    req: TokenRequest,
) -> Result<Json<TokenResponse>, OAuthError> {
    let code = req.code.ok_or(OAuthError::InvalidRequest)?;
    let redirect_uri = req.redirect_uri.ok_or(OAuthError::InvalidRequest)?;
    let code_verifier = req.code_verifier.ok_or(OAuthError::InvalidRequest)?;

    let grant = db::oauth_grants::get(conn, &code)
        .await?
        .ok_or(OAuthError::InvalidGrant)?;

    // Must be an unexpired, not-yet-consumed authorization code.
    if grant.grant_type != "auth_code"
        || grant.status != "approved"
        || grant.expires_at < OffsetDateTime::now_utc()
    {
        return Err(OAuthError::InvalidGrant);
    }
    // Bindings must all match what the code was issued for.
    if grant.redirect_uri.as_deref() != Some(redirect_uri.as_str()) {
        return Err(OAuthError::InvalidGrant);
    }
    if let Some(cid) = &req.client_id {
        if cid != &grant.client_id {
            return Err(OAuthError::InvalidGrant);
        }
    }
    let (challenge, method) = match (&grant.code_challenge, &grant.code_challenge_method) {
        (Some(c), Some(m)) => (c, m),
        _ => return Err(OAuthError::InvalidGrant),
    };
    if !verify_pkce(&code_verifier, challenge, method) {
        return Err(OAuthError::InvalidGrant);
    }
    let account_id = grant.account_id.ok_or(OAuthError::InvalidGrant)?;

    // Single-use: consume before minting so a lost race can't double-issue.
    if db::oauth_grants::consume(conn, &code).await? == 0 {
        return Err(OAuthError::InvalidGrant);
    }

    let access_token = random_token();
    db::project_tokens::create(
        conn,
        &access_token,
        account_id,
        &grant.project_url,
        state.config.project_token_lifetime_secs,
    )
    .await?;

    Ok(Json(TokenResponse {
        access_token,
        token_type: "Bearer",
        expires_in: state.config.project_token_lifetime_secs,
        auth_time: grant.auth_time.map(|t| t.unix_timestamp()),
    }))
}

async fn token_device_code(
    state: &AppState,
    conn: &mut sqlx::PgConnection,
    req: TokenRequest,
) -> Result<Json<TokenResponse>, OAuthError> {
    let device_code = req.device_code.ok_or(OAuthError::InvalidRequest)?;

    let grant = db::oauth_grants::get(conn, &device_code)
        .await?
        .ok_or(OAuthError::InvalidGrant)?;

    if grant.grant_type != "device_code" {
        return Err(OAuthError::InvalidGrant);
    }
    if let Some(cid) = &req.client_id {
        if cid != &grant.client_id {
            return Err(OAuthError::InvalidGrant);
        }
    }
    if grant.expires_at < OffsetDateTime::now_utc() {
        return Err(OAuthError::ExpiredToken);
    }

    // Poll-rate enforcement (RFC 8628 slow_down): compare against the previous
    // poll, then record this one.
    let prev = grant.last_polled_at;
    db::oauth_grants::mark_polled(conn, &device_code).await?;
    if let Some(prev) = prev {
        let elapsed = OffsetDateTime::now_utc() - prev;
        if elapsed.whole_seconds() < state.config.oauth_device_poll_interval_secs {
            return Err(OAuthError::SlowDown);
        }
    }

    match grant.status.as_str() {
        "pending" => Err(OAuthError::AuthorizationPending),
        "denied" => Err(OAuthError::AccessDenied),
        "consumed" => Err(OAuthError::InvalidGrant),
        "approved" => {
            let access_token = grant.access_token.clone().ok_or(OAuthError::Server)?;
            // Single-use: mark consumed so a repeated poll fails.
            if db::oauth_grants::consume(conn, &device_code).await? == 0 {
                return Err(OAuthError::InvalidGrant);
            }
            Ok(Json(TokenResponse {
                access_token,
                token_type: "Bearer",
                expires_in: state.config.project_token_lifetime_secs,
                auth_time: grant.auth_time.map(|t| t.unix_timestamp()),
            }))
        }
        _ => Err(OAuthError::InvalidGrant),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_s256_roundtrip() {
        // Known RFC 7636 Appendix B test vector.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
        assert!(verify_pkce(verifier, challenge, "S256"));
        assert!(!verify_pkce(verifier, challenge, "plain"));
        assert!(!verify_pkce("wrong-verifier", challenge, "S256"));
    }

    #[test]
    fn user_code_format() {
        let c = random_user_code();
        assert_eq!(c.len(), 9);
        assert_eq!(c.as_bytes()[4], b'-');
        assert!(c.chars().all(|ch| ch == '-' || "BCDFGHJKMNPQRSTVWXZ23456789".contains(ch)));
    }

    #[test]
    fn find_client_requires_client_id_and_url() {
        let json = r#"[
            {"name":"NoLogin","url":"https://a.test","description":"x"},
            {"name":"Login","url":"https://b.test","client_id":"cid-b","redirect_uris":["https://b.test/cb"],"official":true}
        ]"#;
        assert!(find_client(json, "cid-a").is_none());
        let c = find_client(json, "cid-b").expect("client b");
        assert_eq!(c.project_url, "https://b.test");
        assert_eq!(c.redirect_uris, vec!["https://b.test/cb".to_string()]);
        assert!(c.official);
    }

    #[test]
    fn urlencode_escapes_reserved() {
        assert_eq!(urlencode("ABCD-EFGH"), "ABCD-EFGH");
        assert_eq!(urlencode("https://x.test/cb?a=b"), "https%3A%2F%2Fx.test%2Fcb%3Fa%3Db");
    }
}
