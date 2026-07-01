//! Project endpoints: list Projects, issue and verify Project tokens.
//!
//! - `GET /v1/projects` — list installed Projects (unauthenticated).
//! - `POST /v1/project-token` — issue a short-lived Project token (authenticated).
//! - `GET /v1/project-token/verify` — verify a Project token (unauthenticated).

use axum::{
    extract::{Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::{db, error::ServerError, middleware::auth::AuthDevice, state::AppState};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/projects", get(list_projects))
        .route("/v1/project-token", post(issue_project_token))
        .route("/v1/project-token/verify", get(verify_project_token))
}

// ── GET /v1/projects ────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct ProjectInfo {
    name: String,
    url: String,
    description: String,
    /// OAuth login client id (docs/25), if this Project supports "Sign in with
    /// Avalanche". Lets clients resolve a login request's `client_id` to this
    /// Project's name/official flag for the consent screen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    client_id: Option<String>,
    /// Server-vouched official flag (docs/54), shown as the verified badge.
    #[serde(default)]
    official: bool,
}

async fn list_projects(
    State(state): State<AppState>,
) -> Result<Json<Vec<ProjectInfo>>, ServerError> {
    let projects: Vec<ProjectInfo> = serde_json::from_str(&state.config.projects_json)
        .map_err(|e| ServerError::Internal(format!("invalid PROJECTS config: {e}")))?;
    Ok(Json(projects))
}

// ── POST /v1/project-token ──────────────────────────────────────────────────

#[derive(Deserialize)]
struct ProjectTokenRequest {
    project_url: String,
}

#[derive(Serialize)]
struct ProjectTokenResponse {
    token: String,
    expires_at: String,
}

async fn issue_project_token(
    State(state): State<AppState>,
    auth: AuthDevice,
    Json(req): Json<ProjectTokenRequest>,
) -> Result<Json<ProjectTokenResponse>, ServerError> {
    let mut conn = state.db.acquire().await?;

    let device = db::devices::find_by_pk(&mut conn, auth.device_pk)
        .await?
        .ok_or(ServerError::Internal("authenticated device not found".into()))?;

    let token = {
        use base64::prelude::*;
        use rand::Rng;
        let bytes: [u8; 32] = rand::rng().random();
        BASE64_URL_SAFE_NO_PAD.encode(bytes)
    };

    let expires_at = db::project_tokens::create(
        &mut conn,
        &token,
        device.account_id,
        &req.project_url,
        state.config.project_token_lifetime_secs,
    )
    .await?;

    Ok(Json(ProjectTokenResponse {
        token,
        expires_at: expires_at.to_string(),
    }))
}

// ── GET /v1/project-token/verify ────────────────────────────────────────────

#[derive(Deserialize)]
struct VerifyQuery {
    token: String,
}

#[derive(Serialize)]
struct VerifyResponse {
    did: String,
    project_url: String,
}

async fn verify_project_token(
    State(state): State<AppState>,
    Query(query): Query<VerifyQuery>,
) -> Result<Json<VerifyResponse>, ServerError> {
    let mut conn = state.db.acquire().await?;

    let (did, project_url) = db::project_tokens::verify(&mut conn, &query.token)
        .await?
        .ok_or(ServerError::Unauthorized)?;

    Ok(Json(VerifyResponse { did, project_url }))
}
