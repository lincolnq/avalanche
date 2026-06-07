//! DID key rotation: `POST /v1/accounts/rotate-key`.
//!
//! Updates the signing (identity) key for a device and syncs the local DID
//! document's `verificationMethod` entry. Authenticated by a P-256 rotation
//! key signature — the same rotation key that authorizes device replacement —
//! rather than a session token, because this operation is typically performed
//! after recovery when the old session is gone.
//!
//! Only `did:plc:` identifiers are supported; `did:local:` has no PLC-backed
//! rotation key list.
//!
//! # Flow
//!
//! 1. Client constructs payload `"rotate-key:{did}:{device_id}:{new_identity_key_base64}"`.
//! 2. Client signs with P-256 rotation key (ECDSA / SHA-256, DER or raw).
//! 3. Client calls this endpoint with the new key, the rotation key, and the signature.
//! 4. Server verifies the signature, then confirms the rotation key is in the
//!    PLC directory's `rotationKeys` list for this DID.
//! 5. Server updates `devices.identity_key` and `did_documents.document` and
//!    appends a row to `key_rotation_log`.

use axum::{extract::State, routing::post, Json, Router};
use base64::prelude::*;
use p256::ecdsa::{signature::Verifier, Signature, VerifyingKey};
use serde::Deserialize;

use crate::{db, error::ServerError, state::AppState};

pub fn routes() -> Router<AppState> {
    Router::new().route("/v1/accounts/rotate-key", post(rotate_key))
}

#[derive(Deserialize)]
struct RotateKeyRequest {
    did: String,
    device_id: i32,
    /// New Ed25519 identity key in libsignal format (0x05 prefix + 32 bytes), base64.
    new_identity_key: String,
    /// P-256 rotation key (SEC1 compressed or uncompressed), base64.
    rotation_key: String,
    /// ECDSA P-256 signature over `"rotate-key:{did}:{device_id}:{new_identity_key_base64}"`.
    /// DER-encoded ASN.1 or raw 64-byte (r||s) — both accepted.
    rotation_key_signature: String,
}

async fn rotate_key(
    State(state): State<AppState>,
    Json(req): Json<RotateKeyRequest>,
) -> Result<axum::http::StatusCode, ServerError> {
    // Only did:plc: DIDs have PLC-backed rotation key lists. Check early so
    // callers get a clear error without needing to supply a valid key/sig.
    if !req.did.starts_with("did:plc:") {
        return Err(ServerError::BadRequest(
            "key rotation requires a did:plc: identifier".into(),
        ));
    }

    // Decode the rotation key and build the verifying key.
    let rotation_key_bytes = BASE64_STANDARD
        .decode(&req.rotation_key)
        .map_err(|_| ServerError::BadRequest("invalid base64 rotation_key".into()))?;
    let verifying_key = VerifyingKey::from_sec1_bytes(&rotation_key_bytes)
        .map_err(|_| ServerError::BadRequest("invalid P-256 rotation key".into()))?;

    // Decode the new identity key.
    let new_identity_key = BASE64_STANDARD
        .decode(&req.new_identity_key)
        .map_err(|_| ServerError::BadRequest("invalid base64 new_identity_key".into()))?;

    // Reconstruct the canonical signed payload.
    let payload = format!(
        "rotate-key:{}:{}:{}",
        req.did, req.device_id, req.new_identity_key,
    );

    // Verify the rotation key signature (accept DER or raw r||s).
    let sig_bytes = BASE64_STANDARD
        .decode(&req.rotation_key_signature)
        .map_err(|_| ServerError::BadRequest("invalid base64 rotation_key_signature".into()))?;
    let signature = Signature::from_der(&sig_bytes)
        .or_else(|_| Signature::from_slice(&sig_bytes))
        .map_err(|_| ServerError::BadRequest("invalid ECDSA signature".into()))?;
    verifying_key
        .verify(payload.as_bytes(), &signature)
        .map_err(|_| ServerError::Unauthorized)?;

    // Confirm the submitted rotation key is authorized in the PLC directory.
    let submitted_compressed = verifying_key.to_encoded_point(true).as_bytes().to_vec();
    let authorized = crate::plc::fetch_rotation_keys_p256(&req.did).await?;
    if !authorized.iter().any(|k| k == &submitted_compressed) {
        tracing::warn!(
            did = %req.did,
            "rotate-key rejected: submitted rotation key not in PLC rotationKeys"
        );
        return Err(ServerError::Unauthorized);
    }

    let mut conn = state.db.acquire().await?;

    // Look up account and target device.
    let account = db::accounts::find_by_did(&mut conn, &req.did)
        .await?
        .ok_or(ServerError::NotFound)?;

    let device = db::devices::find(&mut conn, account.id, req.device_id)
        .await?
        .ok_or(ServerError::NotFound)?;

    // Rate-limit per account (key rotation is rare; 5/hour is generous).
    if !db::rate_limits::check_and_increment(
        &mut conn,
        account.id,
        crate::middleware::rate_limit::ACTION_ROTATE_KEY,
        crate::middleware::rate_limit::LIMIT_ROTATE_KEY,
        crate::middleware::rate_limit::WINDOW_ROTATE_KEY,
    )
    .await?
    {
        return Err(ServerError::RateLimited);
    }

    let old_key = device.identity_key.clone();

    // Update the device's identity key.
    db::devices::update_identity_key(&mut conn, device.id, &new_identity_key).await?;

    // Sync the local DID document's verificationMethod entry for this device.
    if let Some(mut doc) = db::did::find_by_did(&mut conn, &req.did).await? {
        if let Some(vm_arr) = doc
            .get_mut("verificationMethod")
            .and_then(|v| v.as_array_mut())
        {
            let new_key_b64 = BASE64_STANDARD.encode(&new_identity_key);
            for vm in vm_arr.iter_mut() {
                // Update the entry whose id ends with "#key-{device_id}" or, if
                // there is only one entry, update it unconditionally.
                let matches = vm
                    .get("id")
                    .and_then(|v| v.as_str())
                    .map(|id| {
                        id.ends_with(&format!("#key-{}", req.device_id))
                            || id.ends_with("#key-1")
                    })
                    .unwrap_or(false);
                if matches {
                    vm["publicKeyBase64"] = serde_json::Value::String(new_key_b64.clone());
                }
            }
        }
        db::did::upsert_document(&mut conn, account.id, &doc).await?;
    }

    // Append audit log entry.
    db::devices::log_key_rotation(&mut conn, account.id, Some(&old_key), &new_identity_key)
        .await?;

    Ok(axum::http::StatusCode::NO_CONTENT)
}
