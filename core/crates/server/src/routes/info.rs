//! Server info: `GET /v1/info`.
//!
//! Public, unauthenticated. Returns human-readable server metadata
//! including the operator's privacy policy URL (if configured).

use axum::{extract::State, routing::get, Json, Router};
use serde::Serialize;

use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/v1/info", get(info))
}

#[derive(Serialize)]
struct InfoResponse {
    server_name: String,
    privacy_policy_url: Option<String>,
}

async fn info(State(state): State<AppState>) -> Json<InfoResponse> {
    Json(InfoResponse {
        server_name: state.config.server_name.clone(),
        privacy_policy_url: state.config.privacy_policy_url.clone(),
    })
}
