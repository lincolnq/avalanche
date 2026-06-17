//! Abuse report endpoint (docs/12 §3).
//!
//! - `POST /v1/abuse/report` — authenticated; file a content-free report
//!   `{reported_did, reason}` against another account.
//!
//! The report contains NO message content (the server can't read it anyway) —
//! only the reported DID and a reason enum. It is rate-limited per reporter
//! account and persisted for operator review. Cross-server signed forwarding
//! (docs/12 §3) and the enforcement ladder (§4) are deferred until federation
//! lands; in v1 the report stays on the reporter's own homeserver.

use axum::{extract::State, routing::post, Json, Router};
use serde::Deserialize;

use crate::{db, error::ServerError, middleware::auth::AuthDevice, state::AppState};

pub fn routes() -> Router<AppState> {
    Router::new().route("/v1/abuse/report", post(report))
}

#[derive(Deserialize)]
struct ReportRequest {
    reported_did: String,
    reason: String,
}

async fn report(
    State(state): State<AppState>,
    auth: AuthDevice,
    Json(req): Json<ReportRequest>,
) -> Result<axum::http::StatusCode, ServerError> {
    if req.reported_did.is_empty() || req.reported_did.len() > 512 {
        return Err(ServerError::BadRequest("invalid reported_did".into()));
    }
    if !db::abuse::is_valid_reason(&req.reason) {
        return Err(ServerError::BadRequest("invalid reason".into()));
    }

    let mut conn = state.db.acquire().await?;
    let device = db::devices::find_by_pk(&mut conn, auth.device_pk)
        .await?
        .ok_or(ServerError::Internal("device not found for session".into()))?;

    if !db::rate_limits::check_and_increment(
        &mut conn,
        device.account_id,
        crate::middleware::rate_limit::ACTION_ABUSE_REPORT,
        crate::middleware::rate_limit::LIMIT_ABUSE_REPORT,
        crate::middleware::rate_limit::WINDOW_ABUSE_REPORT,
    )
    .await?
    {
        return Err(ServerError::RateLimited);
    }

    db::abuse::insert(&mut conn, &req.reported_did, &req.reason, device.account_id).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}
