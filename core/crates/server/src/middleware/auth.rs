//! Session-token authentication extractor.
//!
//! Routes that require authentication use `AuthDevice` as a handler
//! parameter. Axum calls `FromRequestParts` before the handler runs,
//! which extracts the `Authorization: Bearer <token>` header, validates
//! it against the database, and resolves it to the device's internal PK.
//!
//! # Security notes
//!
//! - If the header is missing, malformed, or the token is expired/unknown,
//!   the request is rejected with 401 before the handler runs.
//! - The token lookup is a database query on every request. This is
//!   acceptable at single-server scale; a cache could be added later.
//! - The extractor returns the internal `device_pk` (bigint), not the
//!   external DID or device_id. Route handlers use this PK to scope all
//!   database operations to the authenticated device.

use axum::{
    extract::FromRequestParts,
    http::request::Parts,
};
use sqlx::Row;

use crate::{db, error::ServerError, state::AppState};

/// Extractor that validates the `Authorization: Bearer <token>` header and
/// resolves it to the authenticated device's internal PK.
pub struct AuthDevice {
    pub device_pk: i64,
}

impl<S> FromRequestParts<S> for AuthDevice
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ServerError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);
        let token = extract_bearer_token(parts)?;

        let mut conn = app_state.db.acquire().await.map_err(ServerError::Db)?;
        let device_pk = db::sessions::validate(&mut conn, &token)
            .await?
            .ok_or(ServerError::Unauthorized)?;

        Ok(AuthDevice { device_pk })
    }
}

/// Extractor for `/v1/admin/*` endpoints. Validates the session token (same as
/// [`AuthDevice`]), then resolves the device's account and rejects unless that
/// account is a bot of the pinned adminbot Project (slug
/// [`crate::config::ADMINBOT_PROJECT_SLUG`]).
///
/// Authority is *membership in that Project*, not a fixed DID — so adminbot may
/// use any DID(s) and rotate freely. The membership is seeded only from
/// operator config (`ADMINBOT_DIDS`) at startup, never via the admin API, which
/// keeps superuser grantable only by the operator (docs/22).
pub struct AuthAdminbot {
    pub device_pk: i64,
    pub account_id: i64,
    pub did: String,
}

impl<S> FromRequestParts<S> for AuthAdminbot
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ServerError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);

        let token = extract_bearer_token(parts)?;
        let mut conn = app_state.db.acquire().await.map_err(ServerError::Db)?;
        let device_pk = db::sessions::validate(&mut conn, &token)
            .await?
            .ok_or(ServerError::Unauthorized)?;

        let row = sqlx::query(
            "SELECT a.id AS account_id, a.did FROM devices d \
             JOIN accounts a ON d.account_id = a.id WHERE d.id = $1",
        )
        .bind(device_pk)
        .fetch_optional(&mut *conn)
        .await
        .map_err(ServerError::Db)?
        .ok_or(ServerError::Unauthorized)?;
        let account_id: i64 = row.get("account_id");
        let did: String = row.get("did");

        // Adminbot authority = membership in the pinned adminbot Project.
        match db::projects::project_for_account(&mut conn, account_id).await? {
            Some((_, slug)) if slug == crate::config::ADMINBOT_PROJECT_SLUG => {}
            _ => return Err(ServerError::Unauthorized),
        }

        Ok(AuthAdminbot {
            device_pk,
            account_id,
            did,
        })
    }
}

fn extract_bearer_token(parts: &Parts) -> Result<String, ServerError> {
    let header = parts
        .headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or(ServerError::Unauthorized)?;

    header
        .strip_prefix("Bearer ")
        .map(|s| s.to_string())
        .ok_or(ServerError::Unauthorized)
}

/// Allow AppState to be extracted from itself (needed for FromRequestParts).
trait FromRef<T> {
    fn from_ref(input: &T) -> Self;
}

impl FromRef<AppState> for AppState {
    fn from_ref(input: &AppState) -> Self {
        input.clone()
    }
}
