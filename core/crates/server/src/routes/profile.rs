//! Encrypted profile blob endpoints:
//!
//! - `PUT /v1/profile` — authenticated, upload the caller's encrypted profile blob.
//! - `GET /v1/profile/{did}` — authenticated, fetch any user's encrypted blob.
//!
//! Both endpoints are authenticated to prevent unauthenticated membership
//! confirmation: an attacker probing `GET /v1/profile/{did}` could otherwise
//! determine whether a DID is registered on this server, which effectively
//! leaks org membership for small activist orgs.
//!
//! Returns 404 identically whether the DID doesn't exist or the account has no
//! profile yet, so authenticated callers cannot distinguish the two cases.

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
        .route("/v1/profile", put(upload_profile))
        .route("/v1/profile/{did}", get(get_profile))
}

// ── PUT /v1/profile ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct UploadProfileRequest {
    encrypted_blob: String, // base64
}

async fn upload_profile(
    State(state): State<AppState>,
    auth: AuthDevice,
    Json(req): Json<UploadProfileRequest>,
) -> Result<axum::http::StatusCode, ServerError> {
    let blob = BASE64_STANDARD
        .decode(&req.encrypted_blob)
        .map_err(|_| ServerError::BadRequest("invalid base64 encrypted_blob".into()))?;

    // Reasonable upper bound — current blob is just a JSON name; future avatar
    // refs add a URL + key but the image itself is uploaded separately.
    if blob.len() > 16 * 1024 {
        return Err(ServerError::BadRequest("encrypted_blob too large".into()));
    }

    let mut conn = state.db.acquire().await?;
    let device = db::devices::find_by_pk(&mut conn, auth.device_pk)
        .await?
        .ok_or(ServerError::Internal("device not found for session".into()))?;

    if !db::rate_limits::check_and_increment(
        &mut conn,
        device.account_id,
        crate::middleware::rate_limit::ACTION_UPDATE_PROFILE,
        crate::middleware::rate_limit::LIMIT_UPDATE_PROFILE,
        crate::middleware::rate_limit::WINDOW_UPDATE_PROFILE,
    )
    .await?
    {
        return Err(ServerError::RateLimited);
    }

    db::profiles::upsert(&mut conn, device.account_id, &blob).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

// ── GET /v1/profile/{did} ───────────────────────────────────────────────────

#[derive(Serialize)]
struct ProfileResponse {
    encrypted_blob: String, // base64
}

async fn get_profile(
    State(state): State<AppState>,
    _auth: AuthDevice,
    Path(did): Path<String>,
) -> Result<Json<ProfileResponse>, ServerError> {
    let mut conn = state.db.acquire().await?;

    // Identical 404 for "no such DID" and "no profile yet" so an authenticated
    // caller cannot distinguish the two cases.
    let account = db::accounts::find_by_did(&mut conn, &did)
        .await?
        .ok_or(ServerError::NotFound)?;
    let blob = db::profiles::get_by_account_id(&mut conn, account.id)
        .await?
        .ok_or(ServerError::NotFound)?;

    Ok(Json(ProfileResponse {
        encrypted_blob: BASE64_STANDARD.encode(blob),
    }))
}
