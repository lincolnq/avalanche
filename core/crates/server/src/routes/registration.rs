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

use crate::{db, error::ServerError, state::AppState};

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
        generate_local_did(&identity_key, &state.config.server_url)
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

    // Create account.
    let account_id =
        db::accounts::create(&mut conn, &did, req.display_name.as_deref(), req.is_bot).await?;

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
    let plc_pub_key =
        decode_did_key_ed25519(&format!("did:key:{pub_key_multibase}")).map_err(|e| {
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

/// Decode a `did:key:z...` (Ed25519) to the raw 32-byte public key.
fn decode_did_key_ed25519(did_key: &str) -> Result<Vec<u8>, String> {
    let z_part = did_key
        .strip_prefix("did:key:z")
        .ok_or("did:key must start with did:key:z")?;

    let bytes = bs58::decode(z_part)
        .into_vec()
        .map_err(|e| format!("base58 decode failed: {e}"))?;

    // Expect multicodec prefix 0xed 0x01 (Ed25519) + 32 bytes.
    if bytes.len() < 2 {
        return Err("did:key payload too short".into());
    }
    if bytes[0] != 0xed || bytes[1] != 0x01 {
        return Err(format!(
            "unexpected multicodec prefix: [{:#04x}, {:#04x}], expected Ed25519 [0xed, 0x01]",
            bytes[0], bytes[1]
        ));
    }

    Ok(bytes[2..].to_vec())
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

fn generate_token() -> String {
    use rand::Rng;
    let bytes: [u8; 32] = rand::rng().random();
    BASE64_URL_SAFE_NO_PAD.encode(bytes)
}
