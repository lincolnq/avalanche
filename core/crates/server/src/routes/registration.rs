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
//! - **DID generation is a local stub.** The `did:plc` is derived from a
//!   SHA-256 hash of the identity key, server URL, and timestamp. It is only
//!   resolvable on this server. Full PLC directory integration ships in
//!   Stage 9.
//! - **Identity key is stored as-is.** The server trusts the client's
//!   self-reported identity key. In the full protocol, the DID document's
//!   verification method would be checked against the PLC directory.
//! - **Prekeys are public material.** The server stores and serves public
//!   key halves; private halves never leave the client.

use axum::{extract::State, routing::post, Json, Router};
use base64::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgConnection;

use crate::{db, error::ServerError, state::AppState};

pub fn routes() -> Router<AppState> {
    Router::new().route("/v1/accounts", post(register))
}

#[derive(Deserialize)]
struct RegisterRequest {
    identity_key: String, // base64
    registration_id: i32,
    device_id: i32,
    signed_prekey: SignedPreKeyUpload,
    one_time_prekeys: Vec<OneTimePreKeyUpload>,
    kyber_prekey: KyberPreKeyUpload,
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
    Json(req): Json<RegisterRequest>,
) -> Result<(axum::http::StatusCode, Json<RegisterResponse>), ServerError> {
    let identity_key = BASE64_STANDARD
        .decode(&req.identity_key)
        .map_err(|_| ServerError::BadRequest("invalid base64 identity_key".into()))?;

    // Generate a did:plc stub: hash identity_key + server_url + timestamp.
    let did = generate_did_plc(&identity_key, &state.config.server_url);

    let mut conn = state.db.acquire().await?;

    // Create account.
    let account_id = db::accounts::create(&mut conn, &did, None, false).await?;

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
            "id": format!("{did}#actnet"),
            "type": "ActnetHomeserver",
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

    Ok((
        axum::http::StatusCode::CREATED,
        Json(RegisterResponse {
            did,
            session_token: token,
            expires_at: expires_at.to_string(),
        }),
    ))
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

fn generate_did_plc(identity_key: &[u8], server_url: &str) -> String {
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
    // did:plc uses base32 lowercase, truncated to 24 chars.
    let encoded = base32::encode(base32::Alphabet::Rfc4648Lower { padding: false }, &hash);
    format!("did:plc:{}", &encoded[..24])
}

fn generate_token() -> String {
    use rand::Rng;
    let bytes: [u8; 32] = rand::rng().random();
    BASE64_URL_SAFE_NO_PAD.encode(bytes)
}
