//! `/v1/admin/*` endpoints — privileged operations gated on adminbot.
//!
//! Adminbot authority is membership in the pinned adminbot Project (slug
//! [`crate::config::ADMINBOT_PROJECT_SLUG`]), enforced by the [`AuthAdminbot`]
//! extractor. These endpoints are how the operator (through adminbot) installs
//! Projects, grants capabilities, and registers gatekeeper signing keys.
//!
//! The catch-up endpoint `GET /v1/admin/events` is the one exception: it is
//! open to any bot session holding `subscribe.account_joined` (not only
//! adminbot), so a self-routing gatekeeper can recover missed join events.

use axum::{
    extract::{Path, Query, State},
    routing::{delete, get, post},
    Json, Router,
};
use base64::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    config::ADMINBOT_PROJECT_SLUG,
    db,
    error::ServerError,
    middleware::auth::{AuthAdminbot, AuthDevice},
    state::AppState,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/admin/ping", get(ping))
        .route("/v1/admin/projects", post(install_project).get(list_projects))
        .route("/v1/admin/projects/{slug}", delete(uninstall_project))
        .route("/v1/admin/projects/{slug}/bots", post(link_bot))
        .route("/v1/admin/projects/{slug}/bots/{bot_did}", delete(unlink_bot))
        .route("/v1/admin/capabilities", post(grant_capability))
        .route(
            "/v1/admin/capabilities/{slug}/{capability}",
            delete(revoke_capability),
        )
        .route("/v1/admin/events", get(get_events))
}

async fn ping(_auth: AuthAdminbot) -> Json<Value> {
    Json(json!({ "ok": true }))
}

// ---- Project install / list / uninstall -----------------------------------

#[derive(Deserialize)]
struct InstallProjectRequest {
    slug: String,
    name: String,
    url: Option<String>,
    /// Existing bot account DIDs to link to the Project. The accounts must
    /// already be registered (this records the link only; it does not provision
    /// the bot's keys).
    #[serde(default)]
    bot_dids: Vec<String>,
}

#[derive(Serialize)]
struct ProjectView {
    slug: String,
    name: String,
    url: Option<String>,
    has_signing_key: bool,
    capabilities: Vec<String>,
    bot_dids: Vec<String>,
    /// True for the pinned adminbot Project: it holds superuser authority via
    /// the pin, not via capability rows (which is why `capabilities` is empty).
    superuser: bool,
}

fn validate_slug(slug: &str) -> Result<(), ServerError> {
    if !(2..=64).contains(&slug.len())
        || !slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(ServerError::BadRequest(
            "slug must be 2–64 chars of [a-z0-9-]".into(),
        ));
    }
    Ok(())
}

async fn install_project(
    State(state): State<AppState>,
    auth: AuthAdminbot,
    Json(req): Json<InstallProjectRequest>,
) -> Result<(axum::http::StatusCode, Json<Value>), ServerError> {
    validate_slug(&req.slug)?;
    if req.slug == ADMINBOT_PROJECT_SLUG {
        return Err(ServerError::BadRequest(
            "the adminbot Project is reserved".into(),
        ));
    }
    if req.name.is_empty() || req.name.len() > 100 {
        return Err(ServerError::BadRequest("name must be 1–100 chars".into()));
    }

    let mut conn = state.db.acquire().await?;
    if db::projects::find_by_slug(&mut conn, &req.slug).await?.is_some() {
        return Err(ServerError::Conflict("project slug already exists".into()));
    }
    let project_id = db::projects::create(&mut conn, &req.slug, &req.name, req.url.as_deref()).await?;

    for did in &req.bot_dids {
        let account = db::accounts::find_by_did(&mut conn, did)
            .await?
            .ok_or_else(|| ServerError::BadRequest(format!("bot account not found: {did}")))?;
        db::projects::link_bot(&mut conn, project_id, account.id).await?;
    }

    tracing::info!(slug = %req.slug, by = %auth.did, "project installed");
    Ok((
        axum::http::StatusCode::CREATED,
        Json(json!({ "slug": req.slug })),
    ))
}

async fn list_projects(
    State(state): State<AppState>,
    _auth: AuthAdminbot,
) -> Result<Json<Value>, ServerError> {
    let mut conn = state.db.acquire().await?;
    let projects = db::projects::list(&mut conn).await?;
    let mut views = Vec::with_capacity(projects.len());
    for p in projects {
        let capabilities = db::capabilities::list(&mut conn, p.id).await?;
        let bot_dids = db::projects::bot_dids(&mut conn, p.id).await?;
        views.push(ProjectView {
            superuser: p.slug == ADMINBOT_PROJECT_SLUG,
            slug: p.slug,
            name: p.name,
            url: p.url,
            has_signing_key: p.signing_public_key.is_some(),
            capabilities,
            bot_dids,
        });
    }
    Ok(Json(json!({ "projects": views })))
}

async fn uninstall_project(
    State(state): State<AppState>,
    auth: AuthAdminbot,
    Path(slug): Path<String>,
) -> Result<Json<Value>, ServerError> {
    if slug == ADMINBOT_PROJECT_SLUG {
        return Err(ServerError::BadRequest(
            "the adminbot Project cannot be uninstalled".into(),
        ));
    }
    let mut conn = state.db.acquire().await?;
    if !db::projects::delete_by_slug(&mut conn, &slug).await? {
        return Err(ServerError::NotFound);
    }
    tracing::info!(%slug, by = %auth.did, "project uninstalled");
    Ok(Json(json!({ "ok": true })))
}

// ---- Bot linking -----------------------------------------------------------

#[derive(Deserialize)]
struct LinkBotRequest {
    bot_did: String,
}

/// Resolve a Project by slug, refusing the reserved adminbot slug (its
/// membership is set only from operator config, never the API — docs/22).
async fn resolve_mutable_project(
    conn: &mut sqlx::PgConnection,
    slug: &str,
) -> Result<i64, ServerError> {
    if slug == ADMINBOT_PROJECT_SLUG {
        return Err(ServerError::BadRequest(
            "adminbot Project membership is set via operator config, not the API".into(),
        ));
    }
    let project = db::projects::find_by_slug(conn, slug)
        .await?
        .ok_or(ServerError::NotFound)?;
    Ok(project.id)
}

async fn link_bot(
    State(state): State<AppState>,
    _auth: AuthAdminbot,
    Path(slug): Path<String>,
    Json(req): Json<LinkBotRequest>,
) -> Result<Json<Value>, ServerError> {
    let mut conn = state.db.acquire().await?;
    let project_id = resolve_mutable_project(&mut conn, &slug).await?;
    let account = db::accounts::find_by_did(&mut conn, &req.bot_did)
        .await?
        .ok_or_else(|| ServerError::BadRequest(format!("bot account not found: {}", req.bot_did)))?;
    db::projects::link_bot(&mut conn, project_id, account.id).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn unlink_bot(
    State(state): State<AppState>,
    _auth: AuthAdminbot,
    Path((slug, bot_did)): Path<(String, String)>,
) -> Result<Json<Value>, ServerError> {
    let mut conn = state.db.acquire().await?;
    resolve_mutable_project(&mut conn, &slug).await?;
    let account = db::accounts::find_by_did(&mut conn, &bot_did)
        .await?
        .ok_or(ServerError::NotFound)?;
    if !db::projects::unlink_bot(&mut conn, account.id).await? {
        return Err(ServerError::NotFound);
    }
    Ok(Json(json!({ "ok": true })))
}

// ---- Capability grant / revoke ---------------------------------------------

#[derive(Deserialize)]
struct GrantCapabilityRequest {
    project_slug: String,
    capability: String,
    /// Base64 (standard) Ed25519 public key. Required when granting
    /// `registration.gatekeeper`; ignored otherwise.
    gatekeeper_public_key: Option<String>,
}

async fn grant_capability(
    State(state): State<AppState>,
    auth: AuthAdminbot,
    Json(req): Json<GrantCapabilityRequest>,
) -> Result<Json<Value>, ServerError> {
    if !db::capabilities::is_known_capability(&req.capability) {
        return Err(ServerError::BadRequest(format!(
            "unknown capability: {}",
            req.capability
        )));
    }
    let mut conn = state.db.acquire().await?;
    let project = db::projects::find_by_slug(&mut conn, &req.project_slug)
        .await?
        .ok_or(ServerError::NotFound)?;

    if req.capability == db::capabilities::REGISTRATION_GATEKEEPER {
        let key_b64 = req.gatekeeper_public_key.as_deref().ok_or_else(|| {
            ServerError::BadRequest(
                "gatekeeper_public_key is required for registration.gatekeeper".into(),
            )
        })?;
        let key = BASE64_STANDARD
            .decode(key_b64)
            .map_err(|_| ServerError::BadRequest("invalid base64 gatekeeper_public_key".into()))?;
        if key.len() != 32 {
            return Err(ServerError::BadRequest(
                "gatekeeper_public_key must be a 32-byte Ed25519 key".into(),
            ));
        }
        db::projects::set_signing_key(&mut conn, project.id, Some(&key)).await?;
    }

    db::capabilities::grant(&mut conn, project.id, &req.capability, &auth.did).await?;
    tracing::info!(slug = %req.project_slug, capability = %req.capability, "capability granted");
    Ok(Json(json!({ "ok": true })))
}

async fn revoke_capability(
    State(state): State<AppState>,
    _auth: AuthAdminbot,
    Path((slug, capability)): Path<(String, String)>,
) -> Result<Json<Value>, ServerError> {
    let mut conn = state.db.acquire().await?;
    let project = db::projects::find_by_slug(&mut conn, &slug)
        .await?
        .ok_or(ServerError::NotFound)?;
    let existed = db::capabilities::revoke(&mut conn, project.id, &capability).await?;
    // Clearing the gatekeeper capability also retires its signing key, so a
    // revoked gatekeeper can no longer admit registrations (fail-closed).
    if capability == db::capabilities::REGISTRATION_GATEKEEPER {
        db::projects::set_signing_key(&mut conn, project.id, None).await?;
    }
    if !existed {
        return Err(ServerError::NotFound);
    }
    Ok(Json(json!({ "ok": true })))
}

// ---- Event catch-up --------------------------------------------------------

#[derive(Deserialize)]
struct EventsQuery {
    #[serde(default)]
    since: i64,
    kind: Option<String>,
}

#[derive(Serialize)]
struct EventView {
    id: i64,
    kind: String,
    did: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    invite_token: Option<String>,
    joined_at_ms: i64,
}

/// Paginated catch-up for missed server events. Open to any bot session holding
/// `subscribe.account_joined` (resolved via its Project), so a self-routing
/// gatekeeper — not only adminbot — can recover join events it missed offline.
async fn get_events(
    State(state): State<AppState>,
    auth: AuthDevice,
    Query(q): Query<EventsQuery>,
) -> Result<Json<Value>, ServerError> {
    let mut conn = state.db.acquire().await?;

    // Resolve the caller's account and check the capability.
    let account_id: i64 = sqlx::query_scalar(
        "SELECT a.id FROM devices d JOIN accounts a ON d.account_id = a.id WHERE d.id = $1",
    )
    .bind(auth.device_pk)
    .fetch_optional(&mut *conn)
    .await
    .map_err(ServerError::Db)?
    .ok_or(ServerError::Unauthorized)?;

    if !db::capabilities::account_has_capability(
        &mut conn,
        account_id,
        db::capabilities::SUBSCRIBE_ACCOUNT_JOINED,
    )
    .await?
    {
        return Err(ServerError::Forbidden(
            "subscribe.account_joined capability required".into(),
        ));
    }

    let kind = q
        .kind
        .as_deref()
        .unwrap_or(db::server_events::KIND_ACCOUNT_JOINED);
    const LIMIT: i64 = 500;
    let events = db::server_events::fetch_since(&mut conn, q.since, kind, LIMIT).await?;
    let views: Vec<EventView> = events
        .into_iter()
        .map(|e| EventView {
            id: e.id,
            kind: e.kind,
            did: e.did,
            invite_token: e.invite_token,
            joined_at_ms: e.joined_at_ms,
        })
        .collect();
    Ok(Json(json!({ "events": views })))
}
