//! Avatar blob endpoints (docs/55).
//!
//! Avatars are small (≤ `avatar_max_size_bytes`, default 64 KiB), long-lived,
//! **overwrite-in-place** encrypted blobs — one per account, one per group —
//! stored on the same off-DB [`BlobStore`](crate::blobstore::BlobStore) as
//! attachments but WITHOUT a TTL or a DB row: the object id is deterministic, so
//! a re-upload replaces the old bytes (storage is bounded by owner count, no GC
//! needed), and existence is simply blob presence.
//!
//! - **Personal** avatar object id = a UUID derived from the account's internal
//!   id; the server derives it (clients never see it). Fetch is by DID and
//!   authenticated, 404 either way (existence hiding, exactly like
//!   `GET /v1/profile/{did}`).
//! - **Group** avatar object id = an opaque UUID the client derives from the
//!   group master key (`crypto::GroupKey::avatar_object_id`). Knowing it *is*
//!   the membership capability, so GET is unauthenticated (like attachment
//!   download) and the server never learns which group a blob belongs to
//!   (docs/03 §3.9). Uploads are session-authed only for rate limiting.
//!
//! Bytes stream through the homeserver via the blob store, which keeps them off
//! Postgres (LocalFs today; an S3 backend slots in behind the same trait). The
//! allocate/presigned-direct-upload dance the attachment path uses is skipped
//! here on purpose — avatars are tiny, so proxying them is negligible.

use axum::{
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, put},
    Router,
};
use sha2::{Digest as _, Sha256};
use uuid::Uuid;

use crate::{db, error::ServerError, middleware::auth::AuthDevice, state::AppState};

/// Body-limit for avatar uploads. A little headroom above the 64 KiB default cap
/// so an operator can raise `AVATAR_MAX_SIZE_BYTES` modestly without also
/// tripping the transport limit; the per-request `avatar_max_size_bytes` check
/// is the authoritative cap.
const AVATAR_BODY_LIMIT: usize = 256 * 1024;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/v1/profile/avatar",
            put(upload_profile_avatar).delete(delete_profile_avatar),
        )
        .route("/v1/profile/avatar/{did}", get(get_profile_avatar))
        .route(
            "/v1/groups/avatar/{id}",
            get(get_group_avatar)
                .put(upload_group_avatar)
                .delete(delete_group_avatar),
        )
        .layer(DefaultBodyLimit::max(AVATAR_BODY_LIMIT))
}

/// Deterministic, server-derived object id for an account's avatar blob. Derived
/// from the internal account id (never exposed to clients). UUID-shaped to
/// satisfy the blob store's id guard; stable, so a re-upload overwrites in place.
fn profile_avatar_object_id(account_id: i64) -> String {
    let mut h = Sha256::new();
    h.update(b"actnet-profile-avatar-id-v1");
    h.update(account_id.to_be_bytes());
    let d = h.finalize();
    let mut b = [0u8; 16];
    b.copy_from_slice(&d[..16]);
    Uuid::from_bytes(b).to_string()
}

async fn account_id_for(state: &AppState, auth: &AuthDevice) -> Result<i64, ServerError> {
    let mut conn = state.db.acquire().await?;
    let device = db::devices::find_by_pk(&mut conn, auth.device_pk)
        .await?
        .ok_or(ServerError::Internal("device not found for session".into()))?;
    Ok(device.account_id)
}

fn check_size(state: &AppState, body: &Bytes) -> Result<(), ServerError> {
    if body.is_empty() {
        return Err(ServerError::BadRequest("empty avatar".into()));
    }
    if body.len() as i64 > state.config.avatar_max_size_bytes {
        return Err(ServerError::BadRequest("avatar too large".into()));
    }
    Ok(())
}

fn avatar_response(blob: Vec<u8>) -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/octet-stream")],
        Body::from(blob),
    )
        .into_response()
}

async fn rate_limit_avatar(state: &AppState, account_id: i64) -> Result<(), ServerError> {
    let mut conn = state.db.acquire().await?;
    if !db::rate_limits::check_and_increment(
        &mut conn,
        account_id,
        crate::middleware::rate_limit::ACTION_AVATAR_UPLOAD,
        crate::middleware::rate_limit::LIMIT_AVATAR_UPLOAD,
        crate::middleware::rate_limit::WINDOW_AVATAR_UPLOAD,
    )
    .await?
    {
        return Err(ServerError::RateLimited);
    }
    Ok(())
}

// ── Personal avatar ──────────────────────────────────────────────────────────

/// `PUT /v1/profile/avatar` — overwrite the caller's avatar blob (ciphertext).
async fn upload_profile_avatar(
    State(state): State<AppState>,
    auth: AuthDevice,
    body: Bytes,
) -> Result<StatusCode, ServerError> {
    check_size(&state, &body)?;
    let account_id = account_id_for(&state, &auth).await?;
    rate_limit_avatar(&state, account_id).await?;
    let id = profile_avatar_object_id(account_id);
    state.blob_store.put(&id, &body).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /v1/profile/avatar/{did}` — fetch a user's avatar ciphertext.
/// Authenticated + identical 404 for "no such DID" and "no avatar", so an
/// authenticated caller cannot use it to confirm membership (mirrors
/// `GET /v1/profile/{did}`).
async fn get_profile_avatar(
    State(state): State<AppState>,
    _auth: AuthDevice,
    Path(did): Path<String>,
) -> Result<Response, ServerError> {
    let mut conn = state.db.acquire().await?;
    let account = db::accounts::find_by_did(&mut conn, &did)
        .await?
        .ok_or(ServerError::NotFound)?;
    let id = profile_avatar_object_id(account.id);
    let blob = state
        .blob_store
        .get(&id)
        .await?
        .ok_or(ServerError::NotFound)?;
    Ok(avatar_response(blob))
}

/// `DELETE /v1/profile/avatar` — clear the caller's avatar (idempotent).
async fn delete_profile_avatar(
    State(state): State<AppState>,
    auth: AuthDevice,
) -> Result<StatusCode, ServerError> {
    let account_id = account_id_for(&state, &auth).await?;
    let id = profile_avatar_object_id(account_id);
    state.blob_store.delete(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Group avatar ─────────────────────────────────────────────────────────────

/// `PUT /v1/groups/avatar/{id}` — overwrite a group's avatar blob. `id` is the
/// opaque, master-key-derived object id; possession of it is the capability.
/// Session-authed only for rate limiting — the server never associates the id
/// with a group or the uploader's membership (docs/03 §3.9).
async fn upload_group_avatar(
    State(state): State<AppState>,
    auth: AuthDevice,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<StatusCode, ServerError> {
    if Uuid::parse_str(&id).is_err() {
        return Err(ServerError::BadRequest("invalid avatar id".into()));
    }
    check_size(&state, &body)?;
    let account_id = account_id_for(&state, &auth).await?;
    rate_limit_avatar(&state, account_id).await?;
    state.blob_store.put(&id, &body).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /v1/groups/avatar/{id}` — fetch a group's avatar ciphertext.
/// Unauthenticated: the unguessable, master-key-derived `id` is the membership
/// capability (mirrors unauthenticated attachment download). 404 for an unknown
/// id hides existence.
async fn get_group_avatar(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, ServerError> {
    let blob = state
        .blob_store
        .get(&id)
        .await?
        .ok_or(ServerError::NotFound)?;
    Ok(avatar_response(blob))
}

/// `DELETE /v1/groups/avatar/{id}` — clear a group's avatar (idempotent). Gated
/// only by knowledge of the capability id (session-authed for rate-limit
/// symmetry); the authoritative "avatar removed" is the `modify_avatar` group
/// action, gated by `modify_title_role`.
async fn delete_group_avatar(
    State(state): State<AppState>,
    _auth: AuthDevice,
    Path(id): Path<String>,
) -> Result<StatusCode, ServerError> {
    state.blob_store.delete(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::profile_avatar_object_id;

    #[test]
    fn profile_avatar_id_is_deterministic_uuid_per_account() {
        let a = profile_avatar_object_id(42);
        let a2 = profile_avatar_object_id(42);
        let b = profile_avatar_object_id(43);
        assert_eq!(a, a2);
        assert_ne!(a, b);
        // Must be a valid UUID (blob store id guard).
        assert!(uuid::Uuid::parse_str(&a).is_ok());
    }
}
