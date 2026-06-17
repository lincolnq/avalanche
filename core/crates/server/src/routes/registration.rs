//! Account registration: `POST /v1/accounts`.
//!
//! Creates a new account with a `did:plc` identifier, registers the first
//! device, stores the device's prekey bundle, and returns a session token.
//! This is the only unauthenticated write endpoint (no token exists yet).
//!
//! # Security notes
//!
//! - **No authentication on registration.** Anyone can create an account.
//!   Rate limiting by IP (not yet implemented) is the primary abuse control.
//! - **DID verification.** When the client provides a DID, the server
//!   verifies it against the PLC directory: the DID must exist and the
//!   `avalanche` verification method must match the client's identity key.
//!   If no DID is provided (tests/bots), the server generates a local stub.
//! - **Prekeys are public material.** The server stores and serves public
//!   key halves; private halves never leave the client.

use axum::{extract::State, routing::post, Json, Router};
use base64::prelude::*;
use libsignal_protocol as signal;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgConnection;

use crate::{
    config::RegistrationMode,
    db,
    error::ServerError,
    invite_token::{self, TokenError, PURPOSE_INVITE},
    middleware::client_ip::ClientIp,
    state::{AppState, WsPush},
};

pub fn routes() -> Router<AppState> {
    Router::new().route("/v1/accounts", post(register))
}

#[derive(Deserialize)]
struct RegisterRequest {
    /// Client-generated DID (from PLC directory). If absent, server generates a stub.
    did: Option<String>,
    identity_key: String, // base64
    registration_id: i32,
    device_id: i32,
    signed_prekey: SignedPreKeyUpload,
    one_time_prekeys: Vec<OneTimePreKeyUpload>,
    kyber_prekey: KyberPreKeyUpload,
    /// Plaintext display name. **Bot accounts only.** Human accounts should
    /// leave this `None` — human display names are exchanged via encrypted
    /// profile bundles (client-to-client), never stored on the server.
    display_name: Option<String>,
    #[serde(default)]
    is_bot: bool,
    /// Optional reserved suffix for the server-generated `did:local:` DID.
    /// Bot accounts only. When set, the resulting DID is `did:local:{did_suffix}`
    /// instead of a random hash. Used by first-party bots (e.g. the adminbot)
    /// that need a well-known identity. Suffix must be lowercase ASCII
    /// alphanumeric, 3–32 chars.
    did_suffix: Option<String>,
    /// Encrypted recovery blob (opaque ciphertext). Contains rotation key +
    /// identity key + server list, encrypted with the user's passkey-derived
    /// symmetric key. Optional — if absent, no recovery is possible.
    recovery_blob: Option<String>, // base64
    /// Encrypted profile blob (opaque ciphertext, AES-256-GCM under the user's
    /// profile key). Optional — accounts without a profile show DID as the
    /// display name to contacts until set via `PUT /v1/profile`.
    encrypted_profile: Option<String>, // base64
    /// Ed25519 signature proving possession of the identity key.
    /// Signs the canonical payload `"register:{did}"` (base64url, no padding).
    /// Required when `did` is provided.
    identity_key_signature: Option<String>, // base64url
    /// Project-signed invite token (docs/24). Required to register under closed
    /// registration unless the caller qualifies for a bootstrap admission arm.
    /// The raw string is passed through to subscribed bots in the
    /// `AccountJoinedEvent` so they can route by its issuer + routing tags.
    invite_token: Option<String>,
}

#[derive(Deserialize)]
struct SignedPreKeyUpload {
    id: i32,
    public_key: String, // base64
    signature: String,  // base64
}

#[derive(Deserialize)]
struct OneTimePreKeyUpload {
    id: i32,
    public_key: String, // base64
}

#[derive(Deserialize)]
struct KyberPreKeyUpload {
    id: i32,
    public_key: String, // base64
    signature: String,  // base64
}

#[derive(Serialize)]
struct RegisterResponse {
    did: String,
    session_token: String,
    expires_at: String,
}

async fn register(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Json(req): Json<RegisterRequest>,
) -> Result<(axum::http::StatusCode, Json<RegisterResponse>), ServerError> {
    {
        let mut conn = state.db.acquire().await?;
        if !db::ip_rate_limits::check_and_increment(
            &mut conn,
            &ip,
            crate::middleware::rate_limit::ACTION_REGISTER,
            crate::middleware::rate_limit::LIMIT_REGISTER,
            crate::middleware::rate_limit::WINDOW_REGISTER,
        )
        .await?
        {
            return Err(ServerError::RateLimited);
        }
    }

    let identity_key = BASE64_STANDARD
        .decode(&req.identity_key)
        .map_err(|_| ServerError::BadRequest("invalid base64 identity_key".into()))?;

    // Human accounts must provide a DID verified against the PLC directory,
    // plus a signature proving possession of the identity key.
    // Bot accounts may omit both.
    let did = if let Some(client_did) = &req.did {
        if !client_did.starts_with("did:plc:") {
            return Err(ServerError::BadRequest("DID must start with did:plc:".into()));
        }
        verify_did_plc(client_did, &identity_key).await?;
        verify_identity_key_signature(client_did, &state.config.server_url, &identity_key, &req.identity_key_signature)?;
        client_did.clone()
    } else if req.is_bot {
        if let Some(suffix) = &req.did_suffix {
            validate_reserved_suffix(suffix)?;
            format!("did:local:{suffix}")
        } else {
            generate_local_did(&identity_key, &state.config.server_url)
        }
    } else {
        return Err(ServerError::BadRequest(
            "did is required for non-bot accounts".into(),
        ));
    };

    if let Some(name) = &req.display_name {
        if name.len() > 100 {
            return Err(ServerError::BadRequest("display_name too long".into()));
        }
    }

    let recovery_blob = req
        .recovery_blob
        .as_deref()
        .map(|b| BASE64_STANDARD.decode(b))
        .transpose()
        .map_err(|_| ServerError::BadRequest("invalid base64 recovery_blob".into()))?;

    let mut conn = state.db.acquire().await?;

    // Closed-registration gating + invite-token validation (docs/24). The
    // server validates a signed gatekeeper token locally against the issuing
    // Project's pinned key, or admits via the operator's shared secret — it
    // never calls the Project. Fails closed. Returns an optional Project to
    // link the new account into (a bootstrap token may name one).
    let link_project = gate_registration(&mut conn, &state, &did, &req).await?;

    // Create account.
    let account_id =
        db::accounts::create(&mut conn, &did, req.display_name.as_deref(), req.is_bot).await?;

    // If the (secret-authorized) bootstrap token named a Project, link the new
    // account into it. Naming the superuser Project is how the operator/adminbot
    // bootstraps superuser authority.
    if let Some(project_id) = link_project {
        db::projects::link_bot(&mut conn, project_id, account_id).await?;
    }

    // Store recovery blob if provided.
    if let Some(blob) = &recovery_blob {
        db::accounts::update_recovery_blob(&mut conn, account_id, Some(blob)).await?;
    }

    // Store encrypted profile blob if provided.
    if let Some(profile_b64) = &req.encrypted_profile {
        let profile_blob = BASE64_STANDARD
            .decode(profile_b64)
            .map_err(|_| ServerError::BadRequest("invalid base64 encrypted_profile".into()))?;
        if profile_blob.len() > 16 * 1024 {
            return Err(ServerError::BadRequest("encrypted_profile too large".into()));
        }
        db::profiles::upsert(&mut conn, account_id, &profile_blob).await?;
    }

    // Create device.
    let device_pk = db::devices::create(
        &mut conn,
        account_id,
        req.device_id,
        &identity_key,
        req.registration_id,
    )
    .await?;

    // Store DID document.
    let did_doc = serde_json::json!({
        "@context": ["https://www.w3.org/ns/did/v1"],
        "id": did,
        "verificationMethod": [{
            "id": format!("{did}#key-1"),
            "type": "Ed25519VerificationKey2020",
            "controller": did,
            "publicKeyBase64": req.identity_key,
        }],
        "service": [{
            "id": format!("{did}#avalanche"),
            "type": "AvalancheHomeserver",
            "serviceEndpoint": state.config.server_url,
        }],
    });
    db::did::upsert_document(&mut conn, account_id, &did_doc).await?;

    // Store prekeys.
    store_prekeys(&mut conn, device_pk, &req).await?;

    // Issue session token.
    let token = generate_token();
    let expires_at =
        db::sessions::create(&mut conn, &token, device_pk, state.config.token_lifetime_secs)
            .await?;

    // Announce the new account to bots holding `subscribe.account_joined`.
    // Two paths: (1) a durable append to `server_events` so a disconnected bot
    // can catch up via `GET /v1/admin/events`; (2) a best-effort live fan-out
    // to every currently-subscribed session. The event carries the raw invite
    // token so bots can route by its issuer + routing tags (docs/22, 24).
    let joined_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    if let Err(e) = db::server_events::append_account_joined(
        &mut conn,
        &did,
        req.invite_token.as_deref(),
        joined_at_ms,
    )
    .await
    {
        // Non-fatal: the account exists; only catch-up is degraded.
        tracing::warn!(%did, "failed to append account_joined server event: {e}");
    }
    {
        let subs = state.account_joined_subscribers.read().await;
        tracing::info!(%did, subscribers = subs.len(), "fanning out AccountJoined");
        for tx in subs.values() {
            let _ = tx.send(WsPush::AccountJoined {
                did: did.clone(),
                joined_at_ms,
                invite_token: req.invite_token.clone(),
            });
        }
    }

    Ok((
        axum::http::StatusCode::CREATED,
        Json(RegisterResponse {
            did,
            session_token: token,
            expires_at: expires_at.to_string(),
        }),
    ))
}

/// Closed-registration admission + invite-token validation.
///
/// Two credentials are accepted:
///   (a) a **signed gatekeeper token** — verified against the issuing Project's
///       pinned key, single-use (`jti` redeemed before account creation);
///   (b) the **bootstrap shared secret** — the operator's setup-time root
///       credential, honored only while no gatekeeper is installed. A bootstrap
///       token may name a Project to link the new account into (e.g. the
///       superuser Project).
///
/// In `Closed` mode (the default) registration is refused unless one of these
/// validates (fail-closed). In `Open` mode (dev) any registration is admitted,
/// but a supplied token is still validated — and a bootstrap token may still
/// link the account into a Project, so the operator/adminbot can claim
/// superuser even in dev.
///
/// Returns the id of a Project to link the new account into (`None` if the
/// token named none or there was no bootstrap token). The server never calls
/// the Project.
async fn gate_registration(
    conn: &mut PgConnection,
    state: &AppState,
    did: &str,
    req: &RegisterRequest,
) -> Result<Option<i64>, ServerError> {
    let mut admitted_by_token = false;
    let mut link_project: Option<i64> = None;

    if let Some(raw) = req.invite_token.as_deref() {
        match invite_token::parse(raw).map_err(map_token_err)? {
            // (a) Signed gatekeeper invite — verify against the pinned key.
            invite_token::ParsedToken::Gatekeeper(envelope) => {
                let project = db::projects::find_by_slug(conn, &envelope.iss)
                    .await?
                    .ok_or_else(|| ServerError::Forbidden("unknown invite token issuer".into()))?;
                if !db::capabilities::project_has(
                    conn,
                    project.id,
                    db::capabilities::REGISTRATION_GATEKEEPER,
                )
                .await?
                {
                    return Err(ServerError::Forbidden(
                        "issuer is not a registration gatekeeper".into(),
                    ));
                }
                let key = project.signing_public_key.ok_or_else(|| {
                    ServerError::Forbidden("gatekeeper has no registered signing key".into())
                })?;
                let claims = invite_token::verify_claims(
                    &envelope,
                    &key,
                    &state.config.server_url,
                    PURPOSE_INVITE,
                    invite_token::now_unix(),
                )
                .map_err(map_token_err)?;

                // Single-use: INSERT-as-gate before account creation. A replay
                // conflicts and is rejected. A token consumed here is spent even
                // if a later step fails — the fail-closed direction.
                let redeemed = db::token_redemptions::try_redeem(
                    conn,
                    &claims.jti,
                    &claims.iss,
                    &claims.purpose,
                    did,
                )
                .await?;
                if !redeemed {
                    return Err(ServerError::Forbidden("invite token already redeemed".into()));
                }
                admitted_by_token = true;
            }

            // (b) Bootstrap shared secret — honored only while no gatekeeper is
            // installed (the secret auto-disables once real vetting exists).
            invite_token::ParsedToken::Bootstrap(boot) => {
                let configured = state.config.registration_shared_secret.as_deref();
                let secret_ok = configured
                    .is_some_and(|s| invite_token::secret_eq(s, &boot.bootstrap_secret));
                let gatekeeper_installed = db::capabilities::any_gatekeeper_exists(conn).await?;

                if secret_ok && !gatekeeper_installed {
                    admitted_by_token = true;
                    // A bootstrap token may name a Project to land in. The admin
                    // API can't link the superuser Project, so this secret-gated
                    // path is the only way to bootstrap superuser authority.
                    if let Some(slug) = boot.project.as_deref() {
                        let project = db::projects::find_by_slug(conn, slug).await?.ok_or_else(
                            || ServerError::Forbidden("bootstrap token names unknown project".into()),
                        )?;
                        link_project = Some(project.id);
                    }
                }
                // Otherwise (wrong secret, or the secret is retired because a
                // gatekeeper is installed) the token doesn't admit and names no
                // project. The mode check below decides the outcome: Closed
                // rejects; Open still admits (with no Project link).
            }
        }
    }

    match state.config.registration_mode {
        RegistrationMode::Open => Ok(link_project),
        RegistrationMode::Closed => {
            if admitted_by_token {
                Ok(link_project)
            } else {
                Err(ServerError::Forbidden(
                    "registration is closed: a valid invite token or shared secret is required"
                        .into(),
                ))
            }
        }
    }
}

/// Map an invite-token validation failure to an HTTP status: structural
/// problems are 400; admission failures are 403 (fail-closed).
fn map_token_err(e: TokenError) -> ServerError {
    match e {
        TokenError::Malformed(m) => ServerError::BadRequest(format!("invalid invite token: {m}")),
        other => ServerError::Forbidden(other.to_string()),
    }
}

async fn store_prekeys(
    conn: &mut PgConnection,
    device_pk: i64,
    req: &RegisterRequest,
) -> Result<(), ServerError> {
    let spk = &req.signed_prekey;
    db::prekeys::upsert_signed(
        conn,
        device_pk,
        spk.id,
        &BASE64_STANDARD
            .decode(&spk.public_key)
            .map_err(|_| ServerError::BadRequest("invalid base64 signed_prekey".into()))?,
        &BASE64_STANDARD
            .decode(&spk.signature)
            .map_err(|_| ServerError::BadRequest("invalid base64 signed_prekey signature".into()))?,
    )
    .await?;

    let otpks: Vec<(i32, Vec<u8>)> = req
        .one_time_prekeys
        .iter()
        .map(|k| {
            Ok((
                k.id,
                BASE64_STANDARD
                    .decode(&k.public_key)
                    .map_err(|_| ServerError::BadRequest("invalid base64 one_time_prekey".into()))?,
            ))
        })
        .collect::<Result<_, ServerError>>()?;
    db::prekeys::insert_one_time_batch(conn, device_pk, &otpks).await?;

    let kpk = &req.kyber_prekey;
    db::prekeys::upsert_kyber(
        conn,
        device_pk,
        kpk.id,
        &BASE64_STANDARD
            .decode(&kpk.public_key)
            .map_err(|_| ServerError::BadRequest("invalid base64 kyber_prekey".into()))?,
        &BASE64_STANDARD
            .decode(&kpk.signature)
            .map_err(|_| ServerError::BadRequest("invalid base64 kyber_prekey signature".into()))?,
    )
    .await?;

    Ok(())
}

/// Verify that the client actually holds the private key for the identity key.
///
/// The client signs `"register:{did}:{server_url}"` with the Ed25519 identity key.
/// This prevents an attacker from registering with someone else's public key,
/// and the server URL binding prevents cross-server replay.
fn verify_identity_key_signature(
    did: &str,
    server_url: &str,
    identity_key_bytes: &[u8],
    signature: &Option<String>,
) -> Result<(), ServerError> {
    let sig_b64 = signature
        .as_deref()
        .ok_or_else(|| ServerError::BadRequest("identity_key_signature is required".into()))?;

    let sig_bytes = BASE64_URL_SAFE_NO_PAD
        .decode(sig_b64)
        .map_err(|_| ServerError::BadRequest("invalid base64 identity_key_signature".into()))?;

    let identity_key = signal::IdentityKey::decode(identity_key_bytes)
        .map_err(|_| ServerError::BadRequest("invalid identity_key".into()))?;

    let payload = format!("register:{did}:{server_url}");
    let valid = identity_key
        .public_key()
        .verify_signature(payload.as_bytes(), &sig_bytes);

    if !valid {
        tracing::warn!(
            "identity_key_signature failed for DID {did} \
             (server_url used in payload: {server_url})"
        );
        return Err(ServerError::BadRequest(
            "identity_key_signature verification failed".into(),
        ));
    }

    Ok(())
}

/// Verify a client-provided DID against the PLC directory.
///
/// Fetches the DID document, finds the `actnet` verification method,
/// decodes the `did:key`, and checks it matches the identity key.
async fn verify_did_plc(did: &str, identity_key: &[u8]) -> Result<(), ServerError> {
    let url = format!("https://plc.directory/{did}");
    let resp = reqwest::get(&url)
        .await
        .map_err(|e| ServerError::Internal(format!("PLC directory request failed: {e}")))?;

    if !resp.status().is_success() {
        return Err(ServerError::BadRequest(format!(
            "DID not found in PLC directory: {did}"
        )));
    }

    let doc: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| ServerError::Internal(format!("PLC directory response parse failed: {e}")))?;

    // The resolved DID document has verificationMethod as an array of objects:
    //   { "id": "did:plc:...#avalanche", "type": "Multikey", "publicKeyMultibase": "z6Mk..." }
    // Find the entry whose id ends with "#avalanche".
    let vm_array = doc
        .get("verificationMethod")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            ServerError::BadRequest("DID document missing verificationMethod array".into())
        })?;

    let avalanche_vm = vm_array
        .iter()
        .find(|vm| {
            vm.get("id")
                .and_then(|id| id.as_str())
                .is_some_and(|id| id.ends_with("#avalanche"))
        })
        .ok_or_else(|| {
            ServerError::BadRequest("DID document missing #avalanche verification method".into())
        })?;

    let pub_key_multibase = avalanche_vm
        .get("publicKeyMultibase")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            ServerError::BadRequest("avalanche verification method missing publicKeyMultibase".into())
        })?;

    // publicKeyMultibase is "z" + base58btc(multicodec_prefix + raw_key).
    // This is the same encoding as did:key without the "did:key:" prefix.
    let plc_pub_key = crate::plc::decode_did_key_ed25519(&format!("did:key:{pub_key_multibase}"))
        .map_err(|e| {
            ServerError::BadRequest(format!("invalid verification method in DID doc: {e}"))
        })?;

    // The client's identity_key is libsignal format: 0x05 prefix + 32 raw bytes.
    // Strip the prefix for comparison.
    let client_raw = if identity_key.len() == 33 && identity_key[0] == 0x05 {
        &identity_key[1..]
    } else {
        identity_key
    };

    if plc_pub_key != client_raw {
        return Err(ServerError::BadRequest(
            "identity key does not match DID document verification method".into(),
        ));
    }

    Ok(())
}

/// Generate a local-only DID for bot accounts that don't use the PLC directory.
/// Uses `did:local:` prefix to make it clear this is not a real PLC DID.
fn generate_local_did(identity_key: &[u8], server_url: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(identity_key);
    hasher.update(server_url.as_bytes());
    hasher.update(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
            .to_le_bytes(),
    );
    let hash = hasher.finalize();
    let encoded = base32::encode(base32::Alphabet::Rfc4648Lower { padding: false }, &hash);
    format!("did:local:{}", &encoded[..24])
}

fn validate_reserved_suffix(suffix: &str) -> Result<(), ServerError> {
    if !(3..=32).contains(&suffix.len())
        || !suffix
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    {
        return Err(ServerError::BadRequest(
            "did_suffix must be 3–32 lowercase alphanumeric chars".into(),
        ));
    }
    Ok(())
}

fn generate_token() -> String {
    use rand::Rng;
    let bytes: [u8; 32] = rand::rng().random();
    BASE64_URL_SAFE_NO_PAD.encode(bytes)
}
