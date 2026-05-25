//! Account info: `GET /v1/accounts/{did}` and `GET /v1/accounts/{did}/devices`.
//!
//! Returns the public metadata for an account — display name and bot flag,
//! and the list of active device_ids. Both endpoints require authentication
//! so they cannot be used for unauthenticated account enumeration.
//!
//! **Note:** `display_name` is only populated for bot accounts. Human display
//! names are exchanged via encrypted profile bundles (client-to-client) and
//! are never stored on the server. Clients should use this endpoint to look up
//! bot names, not human names.

use axum::{extract::{Path, State}, routing::get, Json, Router};
use serde::Serialize;

use crate::{db, error::ServerError, middleware::auth::AuthDevice, state::AppState};

pub fn routes() -> Router<AppState> {
    Router::new()
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
}

/// List the active device_ids for an account. Used by senders to fan-out
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
        device_ids: devices.into_iter().map(|d| d.device_id).collect(),
    }))
}
