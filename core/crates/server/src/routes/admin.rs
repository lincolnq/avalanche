//! `/v1/admin/*` endpoints — privileged operations gated on the pinned
//! `ADMINBOT_DID`.
//!
//! Every route in this module takes [`AuthAdminbot`] as an extractor, so the
//! middleware check (session valid + DID equals `ADMINBOT_DID`) runs before
//! the handler. If the pin isn't set, every request 401s.

use axum::{routing::get, Json, Router};
use serde_json::{json, Value};

use crate::{middleware::auth::AuthAdminbot, state::AppState};

pub fn routes() -> Router<AppState> {
    Router::new().route("/v1/admin/ping", get(ping))
}

async fn ping(_auth: AuthAdminbot) -> Json<Value> {
    Json(json!({ "ok": true }))
}
