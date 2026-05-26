//! Device replacement: `POST /v1/devices/replace`.
//!
//! Authenticated by a **rotation key signature** (not a session token).
//! Used during recovery after device loss: the client presents its DID,
//! a signed payload proving possession of the rotation key, and the new
//! device's credentials. The server revokes the old device (invalidating
//! its session tokens and prekey bundles) and registers the new one.
//!
//! The rotation key is a P-256 keypair. The server verifies the ECDSA
//! signature over a canonical payload containing the DID, old device_id,
//! new device_id, and a server-issued nonce (to prevent replay).
//!
//! # Flow
//!
//! 1. Client calls `POST /v1/auth/challenge` with `{ did, device_id }` using
//!    the **old** device_id to get a nonce.
//! 2. Client constructs the replacement payload and signs it with the rotation key.
//! 3. Client calls `POST /v1/devices/replace` with the signed payload + new device info.
//! 4. Server verifies the rotation key signature, deletes the old device, registers
//!    the new one, and returns a session token.

use axum::{extract::State, routing::post, Json, Router};
use base64::prelude::*;
use p256::ecdsa::{signature::Verifier, Signature, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::{db, error::ServerError, state::AppState};

pub fn routes() -> Router<AppState> {
    Router::new().route("/v1/devices/replace", post(replace_device))
}

#[derive(Deserialize)]
struct ReplaceDeviceRequest {
    did: String,
    old_device_id: i32,
    new_device_id: i32,
    new_identity_key: String,    // base64
    new_registration_id: i32,
    /// The nonce from `POST /v1/auth/challenge` (issued for the old device).
    nonce: String,
    /// ECDSA P-256 signature over the canonical payload:
    /// `"replace:{did}:{old_device_id}:{new_device_id}:{nonce}"`
    rotation_key_signature: String, // base64
    /// The P-256 public key (SEC1 compressed or uncompressed) that signed the payload.
    /// The server verifies this matches the DID document's rotation key.
    rotation_key: String, // base64
    // Prekeys for the new device:
    signed_prekey: SignedPreKeyUpload,
    one_time_prekeys: Vec<OneTimePreKeyUpload>,
    kyber_prekey: KyberPreKeyUpload,
    /// Updated recovery blob (re-encrypted with the new device's state).
    recovery_blob: Option<String>, // base64
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
struct ReplaceDeviceResponse {
    session_token: String,
    expires_at: String,
}

async fn replace_device(
    State(state): State<AppState>,
    Json(req): Json<ReplaceDeviceRequest>,
) -> Result<Json<ReplaceDeviceResponse>, ServerError> {
    // Decode the rotation key.
    let rotation_key_bytes = BASE64_STANDARD
        .decode(&req.rotation_key)
        .map_err(|_| ServerError::BadRequest("invalid base64 rotation_key".into()))?;
    let verifying_key = VerifyingKey::from_sec1_bytes(&rotation_key_bytes)
        .map_err(|_| ServerError::BadRequest("invalid P-256 rotation key".into()))?;

    // Construct the canonical payload that was signed.
    let payload = format!(
        "replace:{}:{}:{}:{}",
        req.did, req.old_device_id, req.new_device_id, req.nonce
    );

    // Verify the rotation key signature.
    let sig_bytes = BASE64_STANDARD
        .decode(&req.rotation_key_signature)
        .map_err(|_| ServerError::BadRequest("invalid base64 rotation_key_signature".into()))?;
    let signature = Signature::from_der(&sig_bytes)
        .or_else(|_| Signature::from_slice(&sig_bytes))
        .map_err(|_| ServerError::BadRequest("invalid ECDSA signature".into()))?;
    verifying_key
        .verify(payload.as_bytes(), &signature)
        .map_err(|_| ServerError::Unauthorized)?;

    // Verify the rotation key is in the PLC directory's authorized
    // rotationKeys list for this DID. Without this check, a valid
    // self-signature proves nothing — anyone can sign with their own
    // freshly generated key.
    //
    // did:local: identifiers (bot accounts) are not in the PLC directory
    // and cannot use this flow.
    if !req.did.starts_with("did:plc:") {
        return Err(ServerError::BadRequest(
            "device replacement requires a did:plc: identifier".into(),
        ));
    }
    let submitted_compressed = verifying_key.to_encoded_point(true).as_bytes().to_vec();
    let authorized = crate::plc::fetch_rotation_keys_p256(&req.did).await?;
    if !authorized.iter().any(|k| k == &submitted_compressed) {
        tracing::warn!(
            did = %req.did,
            "device replace rejected: submitted rotation key not in PLC rotationKeys"
        );
        return Err(ServerError::Unauthorized);
    }

    let mut conn = state.db.acquire().await?;

    // Look up the account and old device.
    let account = db::accounts::find_by_did(&mut conn, &req.did)
        .await?
        .ok_or(ServerError::NotFound)?;

    let old_device = db::devices::find(&mut conn, account.id, req.old_device_id)
        .await?
        .ok_or(ServerError::NotFound)?;

    // Consume the auth challenge nonce (must have been issued for the old device).
    let challenge_device_pk = db::challenges::consume(&mut conn, &req.nonce)
        .await?
        .ok_or(ServerError::Unauthorized)?;

    if challenge_device_pk != old_device.id {
        return Err(ServerError::Unauthorized);
    }

    // Decode the new identity key.
    let new_identity_key = BASE64_STANDARD
        .decode(&req.new_identity_key)
        .map_err(|_| ServerError::BadRequest("invalid base64 new_identity_key".into()))?;

    // Delete the old device (cascades to tokens, prekeys, messages).
    db::devices::delete(&mut conn, old_device.id).await?;

    // Register the new device.
    let new_device_pk = db::devices::create(
        &mut conn,
        account.id,
        req.new_device_id,
        &new_identity_key,
        req.new_registration_id,
    )
    .await?;

    // Store prekeys for the new device.
    store_prekeys(&mut conn, new_device_pk, &req).await?;

    // Update recovery blob if provided.
    if let Some(blob_b64) = &req.recovery_blob {
        let blob = BASE64_STANDARD
            .decode(blob_b64)
            .map_err(|_| ServerError::BadRequest("invalid base64 recovery_blob".into()))?;
        db::accounts::update_recovery_blob(&mut conn, account.id, Some(&blob)).await?;
    }

    // Issue a session token for the new device.
    let token = generate_token();
    let expires_at =
        db::sessions::create(&mut conn, &token, new_device_pk, state.config.token_lifetime_secs)
            .await?;

    Ok(Json(ReplaceDeviceResponse {
        session_token: token,
        expires_at: expires_at.to_string(),
    }))
}

async fn store_prekeys(
    conn: &mut sqlx::PgConnection,
    device_pk: i64,
    req: &ReplaceDeviceRequest,
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

fn generate_token() -> String {
    use rand::Rng;
    let bytes: [u8; 32] = rand::rng().random();
    BASE64_URL_SAFE_NO_PAD.encode(bytes)
}
