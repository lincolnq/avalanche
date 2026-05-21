//! Device authentication: `POST /v1/auth/challenge` and `POST /v1/auth/token`.
//!
//! Authentication is a two-step challenge-response protocol:
//!
//! 1. **`POST /v1/auth/challenge`** — client submits its DID and device_id;
//!    the server looks up the device, generates a short-lived random nonce,
//!    stores it in `auth_challenges`, and returns it to the client.
//!
//! 2. **`POST /v1/auth/token`** — client signs the nonce bytes with its Ed25519
//!    identity key and submits `{ did, device_id, nonce, signature }`. The
//!    server consumes the nonce (atomically deleting it, so it cannot be
//!    replayed), verifies the signature against the public key stored at
//!    registration, and issues a time-limited session token on success.
//!
//! # Security properties
//!
//! - **Single-use nonces**: `auth_challenges` rows are deleted on first
//!   redemption. A replayed nonce returns 401.
//! - **Short TTL**: challenges expire after 5 minutes.
//! - **Bound to device**: the nonce is stored against a `device_pk`; the
//!   token request cross-checks that the nonce belongs to the claimed device.
//! - **Ed25519 signature**: `libsignal_protocol::IdentityKey::public_key().
//!   verify_signature()` verifies against the stored public key bytes.
//! - **Session tokens** are 256-bit random strings, revocable by deletion.

use axum::{extract::State, routing::post, Json, Router};
use base64::prelude::*;
use libsignal_protocol as signal;
use serde::{Deserialize, Serialize};

use crate::{db, error::ServerError, state::AppState};

const CHALLENGE_LIFETIME_SECS: i64 = 300; // 5 minutes

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/auth/challenge", post(issue_challenge))
        .route("/v1/auth/token", post(issue_token))
}

// ── Challenge ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ChallengeRequest {
    did: String,
    device_id: i32,
}

#[derive(Serialize)]
struct ChallengeResponse {
    nonce: String,
}

async fn issue_challenge(
    State(state): State<AppState>,
    Json(req): Json<ChallengeRequest>,
) -> Result<Json<ChallengeResponse>, ServerError> {
    let mut conn = state.db.acquire().await?;

    let device = db::devices::find_by_did(&mut conn, &req.did, req.device_id)
        .await?
        .ok_or(ServerError::NotFound)?;

    let nonce = {
        use rand::Rng;
        let bytes: [u8; 32] = rand::rng().random();
        BASE64_URL_SAFE_NO_PAD.encode(bytes)
    };

    db::challenges::create(&mut conn, &nonce, device.id, CHALLENGE_LIFETIME_SECS).await?;

    Ok(Json(ChallengeResponse { nonce }))
}

// ── Token ─────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct TokenRequest {
    did: String,
    device_id: i32,
    nonce: String,
    signature: String,
}

#[derive(Serialize)]
struct TokenResponse {
    session_token: String,
    expires_at: String,
}

async fn issue_token(
    State(state): State<AppState>,
    Json(req): Json<TokenRequest>,
) -> Result<Json<TokenResponse>, ServerError> {
    let mut conn = state.db.acquire().await?;

    let device = db::devices::find_by_did(&mut conn, &req.did, req.device_id)
        .await?
        .ok_or(ServerError::NotFound)?;

    // Consume the nonce atomically. Returns None if expired or already used.
    let challenge_device_pk = db::challenges::consume(&mut conn, &req.nonce)
        .await?
        .ok_or(ServerError::Unauthorized)?;

    // The nonce must have been issued for this exact device.
    if challenge_device_pk != device.id {
        return Err(ServerError::Unauthorized);
    }

    let nonce_bytes = BASE64_URL_SAFE_NO_PAD
        .decode(&req.nonce)
        .map_err(|_| ServerError::BadRequest("invalid nonce encoding".into()))?;

    let sig_bytes = BASE64_URL_SAFE_NO_PAD
        .decode(&req.signature)
        .map_err(|_| ServerError::BadRequest("invalid signature encoding".into()))?;

    let identity_key = signal::IdentityKey::decode(&device.identity_key)
        .map_err(|_| ServerError::Internal("corrupt identity key in database".into()))?;

    let valid = identity_key
        .public_key()
        .verify_signature(&nonce_bytes, &sig_bytes);

    if !valid {
        return Err(ServerError::Unauthorized);
    }

    let token = {
        use rand::Rng;
        let bytes: [u8; 32] = rand::rng().random();
        BASE64_URL_SAFE_NO_PAD.encode(bytes)
    };

    let expires_at =
        db::sessions::create(&mut conn, &token, device.id, state.config.token_lifetime_secs)
            .await?;

    Ok(Json(TokenResponse {
        session_token: token,
        expires_at: expires_at.to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use libsignal_protocol as signal;
    use rand::{Rng, TryRngCore as _};

    fn generate_keypair() -> signal::IdentityKeyPair {
        signal::IdentityKeyPair::generate(&mut rand::rngs::OsRng.unwrap_err())
    }

    #[test]
    fn valid_signature_accepted() {
        let keypair = generate_keypair();
        let nonce_bytes: [u8; 32] = rand::rng().random();

        let sig = keypair
            .private_key()
            .calculate_signature(&nonce_bytes, &mut rand::rngs::OsRng.unwrap_err())
            .expect("sign");

        let valid = keypair.identity_key().public_key().verify_signature(&nonce_bytes, &sig);
        assert!(valid);
    }

    #[test]
    fn wrong_signature_rejected() {
        let keypair = generate_keypair();
        let nonce_bytes: [u8; 32] = rand::rng().random();
        let wrong_sig = vec![0u8; 64];

        let valid = keypair.identity_key().public_key().verify_signature(&nonce_bytes, &wrong_sig);
        assert!(!valid);
    }

    #[test]
    fn round_trip_identity_key_serialization() {
        let keypair = generate_keypair();
        let serialized = keypair.identity_key().serialize().to_vec();
        let decoded = signal::IdentityKey::decode(&serialized).expect("decode");
        assert_eq!(decoded.serialize(), keypair.identity_key().serialize());
    }
}
