//! Invite token validation: `GET /v1/invites/:token`.
//!
//! Decodes a base64url invite token, validates its format, and returns
//! server metadata for the client's onboarding flow. Unauthenticated —
//! anyone with a token can check it.

use axum::{extract::State, routing::get, Json, Router};
use base64::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{error::ServerError, state::AppState};

pub fn routes() -> Router<AppState> {
    Router::new().route("/v1/invites/{token}", get(validate_invite))
}

#[derive(Deserialize)]
struct InviteTokenPayload {
    server_url: String,
    inviter_did: Option<String>,
}

#[derive(Serialize)]
struct InviteResponse {
    server_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    post_onboarding_redirect: Option<String>,
}

async fn validate_invite(
    State(state): State<AppState>,
    axum::extract::Path(token): axum::extract::Path<String>,
) -> Result<Json<InviteResponse>, ServerError> {
    let json_bytes = BASE64_URL_SAFE_NO_PAD
        .decode(&token)
        .map_err(|_| ServerError::BadRequest("invalid base64url token".into()))?;

    let payload: InviteTokenPayload = serde_json::from_slice(&json_bytes)
        .map_err(|_| ServerError::BadRequest("invalid token JSON".into()))?;

    // Verify the token is for this server.
    if payload.server_url.trim_end_matches('/') != state.config.server_url.trim_end_matches('/') {
        return Err(ServerError::BadRequest("token is for a different server".into()));
    }

    // If inviter_did is present, generate a post-onboarding redirect to open a DM.
    let post_onboarding_redirect = payload.inviter_did.map(|did| {
        format!("https://{}/conversation/{}", state.config.invite_domain, did)
    });

    Ok(Json(InviteResponse {
        server_name: state.config.server_name.clone(),
        post_onboarding_redirect,
    }))
}
