//! Prekey management endpoints.
//!
//! - `PUT /v1/prekeys` — upload new prekeys (signed, one-time, Kyber).
//! - `GET /v1/prekeys/status` — check remaining pool counts.
//! - `GET /v1/prekeys/{did}/{device_id}` — fetch a device's prekey bundle
//!   for session initiation. Atomically consumes one one-time prekey.
//!
//! # Security notes
//!
//! - **Upload is scoped to the authenticated device.** A client can only
//!   upload prekeys for its own device, enforced by the `AuthDevice`
//!   extractor.
//! - **Fetch is authenticated but not restricted by recipient.** Any
//!   authenticated user can fetch any other user's bundle — this is required
//!   for session initiation.
//! - **One-time prekey consumption is atomic.** The SQL uses
//!   `DELETE ... RETURNING` so concurrent fetches each consume a different
//!   key, preventing key reuse.
//! - **All key material is public.** The server stores and serves public key
//!   halves only.

use axum::{
    extract::{Path, State},
    routing::{get, put},
    Json, Router,
};
use base64::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{db, error::ServerError, middleware::auth::AuthDevice, state::AppState};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/prekeys", put(upload))
        .route("/v1/prekeys/status", get(status))
        .route("/v1/prekeys/{did}/{device_id}", get(fetch))
}

// ── Upload ───────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct UploadRequest {
    signed_prekey: Option<SignedPreKeyUpload>,
    one_time_prekeys: Option<Vec<OneTimePreKeyUpload>>,
    kyber_prekey: Option<KyberPreKeyUpload>,
}

#[derive(Deserialize)]
struct SignedPreKeyUpload {
    id: i32,
    public_key: String,
    signature: String,
}

#[derive(Deserialize)]
struct OneTimePreKeyUpload {
    id: i32,
    public_key: String,
}

#[derive(Deserialize)]
struct KyberPreKeyUpload {
    id: i32,
    public_key: String,
    signature: String,
}

async fn upload(
    State(state): State<AppState>,
    auth: AuthDevice,
    Json(req): Json<UploadRequest>,
) -> Result<(), ServerError> {
    let mut conn = state.db.acquire().await?;

    let device = db::devices::find_by_pk(&mut conn, auth.device_pk)
        .await?
        .ok_or(ServerError::Unauthorized)?;

    if !db::rate_limits::check_and_increment(
        &mut conn,
        device.account_id,
        crate::middleware::rate_limit::ACTION_UPLOAD_PREKEYS,
        crate::middleware::rate_limit::LIMIT_UPLOAD_PREKEYS,
        crate::middleware::rate_limit::WINDOW_UPLOAD_PREKEYS,
    )
    .await?
    {
        return Err(ServerError::RateLimited);
    }

    if let Some(spk) = &req.signed_prekey {
        db::prekeys::upsert_signed(
            &mut conn,
            auth.device_pk,
            spk.id,
            &decode_b64(&spk.public_key)?,
            &decode_b64(&spk.signature)?,
        )
        .await?;
    }

    if let Some(otpks) = &req.one_time_prekeys {
        let keys: Vec<(i32, Vec<u8>)> = otpks
            .iter()
            .map(|k| Ok((k.id, decode_b64(&k.public_key)?)))
            .collect::<Result<_, ServerError>>()?;
        db::prekeys::insert_one_time_batch(&mut conn, auth.device_pk, &keys).await?;
    }

    if let Some(kpk) = &req.kyber_prekey {
        db::prekeys::upsert_kyber(
            &mut conn,
            auth.device_pk,
            kpk.id,
            &decode_b64(&kpk.public_key)?,
            &decode_b64(&kpk.signature)?,
        )
        .await?;
    }

    Ok(())
}

// ── Status ───────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct StatusResponse {
    one_time_remaining: i64,
    kyber_remaining: i64,
}

async fn status(
    State(state): State<AppState>,
    auth: AuthDevice,
) -> Result<Json<StatusResponse>, ServerError> {
    let mut conn = state.db.acquire().await?;
    let one_time = db::prekeys::one_time_count(&mut conn, auth.device_pk).await?;
    let kyber = db::prekeys::kyber_count(&mut conn, auth.device_pk).await?;

    Ok(Json(StatusResponse {
        one_time_remaining: one_time,
        kyber_remaining: kyber,
    }))
}

// ── Fetch bundle ─────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct BundleResponse {
    identity_key: String,
    registration_id: i32,
    signed_prekey: SignedPreKeyWire,
    one_time_prekey: Option<OneTimePreKeyWire>,
    kyber_prekey: KyberPreKeyWire,
}

#[derive(Serialize)]
struct SignedPreKeyWire {
    id: i32,
    public_key: String,
    signature: String,
}

#[derive(Serialize)]
struct OneTimePreKeyWire {
    id: i32,
    public_key: String,
}

#[derive(Serialize)]
struct KyberPreKeyWire {
    id: i32,
    public_key: String,
    signature: String,
}

async fn fetch(
    State(state): State<AppState>,
    auth: AuthDevice,
    Path((did, device_id)): Path<(String, i32)>,
) -> Result<Json<BundleResponse>, ServerError> {
    let mut conn = state.db.acquire().await?;

    let requester = db::devices::find_by_pk(&mut conn, auth.device_pk)
        .await?
        .ok_or(ServerError::Unauthorized)?;

    if !db::rate_limits::check_and_increment(
        &mut conn,
        requester.account_id,
        crate::middleware::rate_limit::ACTION_FETCH_BUNDLE,
        crate::middleware::rate_limit::LIMIT_FETCH_BUNDLE,
        crate::middleware::rate_limit::WINDOW_FETCH_BUNDLE,
    )
    .await?
    {
        return Err(ServerError::RateLimited);
    }

    let device = db::devices::find_by_did(&mut conn, &did, device_id)
        .await?
        .ok_or(ServerError::NotFound)?;

    let bundle = db::prekeys::fetch_bundle(&mut conn, device.id)
        .await?
        .ok_or(ServerError::NotFound)?;

    Ok(Json(BundleResponse {
        identity_key: BASE64_STANDARD.encode(&bundle.identity_key),
        registration_id: bundle.registration_id,
        signed_prekey: SignedPreKeyWire {
            id: bundle.signed_prekey.id,
            public_key: BASE64_STANDARD.encode(&bundle.signed_prekey.public_key),
            signature: BASE64_STANDARD.encode(&bundle.signed_prekey.signature),
        },
        one_time_prekey: bundle.one_time_prekey.map(|k| OneTimePreKeyWire {
            id: k.id,
            public_key: BASE64_STANDARD.encode(&k.public_key),
        }),
        kyber_prekey: KyberPreKeyWire {
            id: bundle.kyber_prekey.id,
            public_key: BASE64_STANDARD.encode(&bundle.kyber_prekey.public_key),
            signature: BASE64_STANDARD.encode(&bundle.kyber_prekey.signature),
        },
    }))
}

fn decode_b64(s: &str) -> Result<Vec<u8>, ServerError> {
    BASE64_STANDARD
        .decode(s)
        .map_err(|_| ServerError::BadRequest("invalid base64".into()))
}
