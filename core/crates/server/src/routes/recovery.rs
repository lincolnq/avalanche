//! Recovery endpoints:
//!
//! - `GET /v1/recovery/{did}` — unauthenticated, returns the encrypted recovery blob
//! - `PUT /v1/recovery` — authenticated, updates the recovery blob (e.g. after joining a new server)
//!
//! The recovery blob is opaque ciphertext decryptable only by the user's passkey.
//! Serving it publicly is safe — an attacker who obtains it cannot decrypt it
//! without the passkey's PRF-derived symmetric key.

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
        .route("/v1/recovery/{did}", get(get_recovery_blob))
        .route("/v1/recovery", put(update_recovery_blob))
}

// ── GET /v1/recovery/{did} ──────────────────────────────────────────────────

#[derive(Serialize)]
struct RecoveryBlobResponse {
    recovery_blob: String, // base64
    /// Active device_ids for the account, returned alongside the blob so the
    /// recovering client can target the correct old device in its replace
    /// request without an extra authenticated round-trip. Safe to expose
    /// here: this endpoint is already gated by knowing the DID, and the
    /// device list is just small integers with no key material.
    device_ids: Vec<i32>,
}

async fn get_recovery_blob(
    State(state): State<AppState>,
    Path(did): Path<String>,
) -> Result<Json<RecoveryBlobResponse>, ServerError> {
    let mut conn = state.db.acquire().await?;
    let blob = db::accounts::get_recovery_blob(&mut conn, &did)
        .await?
        .ok_or(ServerError::NotFound)?;
    let devices = db::devices::list_by_did(&mut conn, &did).await?;
    Ok(Json(RecoveryBlobResponse {
        recovery_blob: BASE64_STANDARD.encode(blob),
        device_ids: devices.into_iter().map(|d| d.device_id).collect(),
    }))
}

// ── PUT /v1/recovery ────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct UpdateRecoveryBlobRequest {
    recovery_blob: String, // base64
}

async fn update_recovery_blob(
    State(state): State<AppState>,
    auth: AuthDevice,
    Json(req): Json<UpdateRecoveryBlobRequest>,
) -> Result<axum::http::StatusCode, ServerError> {
    let blob = BASE64_STANDARD
        .decode(&req.recovery_blob)
        .map_err(|_| ServerError::BadRequest("invalid base64 recovery_blob".into()))?;

    let mut conn = state.db.acquire().await?;
    let device = db::devices::find_by_pk(&mut conn, auth.device_pk)
        .await?
        .ok_or(ServerError::Internal("device not found for session".into()))?;

    if !db::rate_limits::check_and_increment(
        &mut conn,
        device.account_id,
        crate::middleware::rate_limit::ACTION_UPDATE_RECOVERY,
        crate::middleware::rate_limit::LIMIT_UPDATE_RECOVERY,
        crate::middleware::rate_limit::WINDOW_UPDATE_RECOVERY,
    )
    .await?
    {
        return Err(ServerError::RateLimited);
    }

    db::accounts::update_recovery_blob(&mut conn, device.account_id, Some(&blob)).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}
