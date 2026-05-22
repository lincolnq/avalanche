//! Account info: `GET /v1/accounts/{did}`.
//!
//! Returns the public metadata for an account — display name and bot flag.
//! Requires authentication so the endpoint cannot be used for unauthenticated
//! account enumeration.
//!
//! **Note:** `display_name` is only populated for bot accounts. Human display
//! names are exchanged via encrypted profile bundles (client-to-client) and
//! are never stored on the server. Clients should use this endpoint to look up
//! bot names, not human names.

use axum::{extract::{Path, State}, routing::get, Json, Router};
use serde::Serialize;

use crate::{db, error::ServerError, middleware::auth::AuthDevice, state::AppState};

pub fn routes() -> Router<AppState> {
    Router::new().route("/v1/accounts/{did}", get(get_account_info))
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
