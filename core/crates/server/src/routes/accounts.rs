//! Account info: `GET /v1/accounts/{did}` and `GET /v1/accounts/{did}/devices`.
//! Account self-deletion: `DELETE /v1/accounts`.
//!
//! Returns the public metadata for an account — display name and bot flag,
//! and the list of active device_ids. Both GET endpoints require authentication
//! so they cannot be used for unauthenticated account enumeration.
//!
//! **Note:** `display_name` is only populated for bot accounts. Human display
//! names are exchanged via encrypted profile bundles (client-to-client) and
//! are never stored on the server. Clients should use this endpoint to look up
//! bot names, not human names.
//!
//! `DELETE /v1/accounts` permanently deletes the calling account and all its
//! data (devices, prekeys, session tokens, message queue, DID document, push
//! pseudonyms, profile, and rate-limit counters) in a single transaction.
//! Returns 204 No Content on success. Required by App Store guideline 5.1.1(v).

use axum::{extract::{Path, State}, http::StatusCode, routing::{delete, get}, Json, Router};
use serde::Serialize;
use sqlx::Row;

use crate::{db, error::ServerError, middleware::auth::AuthDevice, state::AppState};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/accounts", delete(delete_account_handler))
        .route("/v1/accounts/{did}", get(get_account_info))
        .route("/v1/accounts/{did}/devices", get(list_devices))
}

#[derive(Serialize)]
struct AccountInfoResponse {
    did: String,
    display_name: Option<String>,
    is_bot: bool,
}

async fn get_account_info(
    State(state): State<AppState>,
    _auth: AuthDevice,
    Path(did): Path<String>,
) -> Result<Json<AccountInfoResponse>, ServerError> {
    let mut conn = state.db.acquire().await?;
    let account = db::accounts::find_by_did(&mut conn, &did)
        .await?
        .ok_or(ServerError::NotFound)?;
    Ok(Json(AccountInfoResponse {
        did: account.did,
        display_name: account.display_name,
        is_bot: account.is_bot,
    }))
}

#[derive(Serialize)]
struct DevicesResponse {
    device_ids: Vec<i32>,
    /// Same devices as `device_ids`, each paired with its current
    /// `registration_id`. Senders use the registration id to detect a session
    /// that has gone stale because the peer re-registered the device (the
    /// sealed-sender group-send path can't surface a stale-device error, so
    /// the sender reconciles client-side). Kept alongside `device_ids` so
    /// existing consumers that only need the id list are unaffected.
    devices: Vec<DeviceEntry>,
}

#[derive(Serialize)]
struct DeviceEntry {
    device_id: i32,
    registration_id: i32,
}

/// List the active devices for an account. Used by senders to fan-out
/// encrypted message envelopes across all of the recipient's devices.
async fn list_devices(
    State(state): State<AppState>,
    _auth: AuthDevice,
    Path(did): Path<String>,
) -> Result<Json<DevicesResponse>, ServerError> {
    let mut conn = state.db.acquire().await?;
    let devices = db::devices::list_by_did(&mut conn, &did).await?;
    if devices.is_empty() {
        return Err(ServerError::NotFound);
    }
    Ok(Json(DevicesResponse {
        device_ids: devices.iter().map(|d| d.device_id).collect(),
        devices: devices
            .into_iter()
            .map(|d| DeviceEntry {
                device_id: d.device_id,
                registration_id: d.registration_id,
            })
            .collect(),
    }))
}

/// `DELETE /v1/accounts` — permanently delete the authenticated account and all its data.
///
/// Hard-deletes in a single transaction: devices, prekeys (signed/one-time/kyber),
/// session tokens, auth challenges, message queue, push pseudonyms, project tokens,
/// DID document, profile, rate-limit counters, and the account row itself.
///
/// Returns 204 No Content on success. After this call any Bearer token that was
/// issued to this account will return 401 on subsequent authenticated requests.
async fn delete_account_handler(
    State(state): State<AppState>,
    auth: AuthDevice,
) -> Result<StatusCode, ServerError> {
    let mut conn = state.db.acquire().await?;

    // Resolve device_pk → account_id.
    let row = sqlx::query("SELECT account_id FROM devices WHERE id = $1")
        .bind(auth.device_pk)
        .fetch_optional(&mut *conn)
        .await?;

    let account_id: i64 = row
        .ok_or(ServerError::Unauthorized)?
        .get("account_id");

    db::accounts::delete_account(&mut conn, account_id).await?;

    Ok(StatusCode::NO_CONTENT)
}
