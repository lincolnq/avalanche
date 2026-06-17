//! Project capability grants (docs/22 §Project-capabilities).
//!
//! Capabilities are named, server-enforced permissions attached to a Project
//! (not directly to a bot account). The only grantor is adminbot; `granted_by`
//! records its DID for later cross-referencing with the #admins chat thread.
//!
//! Authority resolution for a *bot account* runs account -> Project ->
//! capability, with one exception: a bot in the pinned adminbot Project is a
//! superuser and implicitly holds every capability (the pin), so no rows are
//! ever seeded for it — a capability row always means an explicit grant.

use sqlx::{PgConnection, Row};

use crate::config::ADMINBOT_PROJECT_SLUG;

/// Known capability strings. Grants are validated against this set so a typo
/// can't create a permanently-dangling permission.
pub const REGISTRATION_GATEKEEPER: &str = "registration.gatekeeper";
pub const SUBSCRIBE_ACCOUNT_JOINED: &str = "subscribe.account_joined";

pub fn is_known_capability(cap: &str) -> bool {
    matches!(cap, REGISTRATION_GATEKEEPER | SUBSCRIBE_ACCOUNT_JOINED)
}

/// Grant a capability to a Project. Idempotent (no error on re-grant).
pub async fn grant(
    conn: &mut PgConnection,
    project_id: i64,
    capability: &str,
    granted_by: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO project_capabilities (project_id, capability, granted_by)
         VALUES ($1, $2, $3)
         ON CONFLICT (project_id, capability) DO NOTHING",
    )
    .bind(project_id)
    .bind(capability)
    .bind(granted_by)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Revoke a capability from a Project. Returns whether a grant existed.
pub async fn revoke(
    conn: &mut PgConnection,
    project_id: i64,
    capability: &str,
) -> Result<bool, sqlx::Error> {
    let res = sqlx::query(
        "DELETE FROM project_capabilities WHERE project_id = $1 AND capability = $2",
    )
    .bind(project_id)
    .bind(capability)
    .execute(&mut *conn)
    .await?;
    Ok(res.rows_affected() > 0)
}

/// List a Project's granted capabilities.
pub async fn list(conn: &mut PgConnection, project_id: i64) -> Result<Vec<String>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT capability FROM project_capabilities WHERE project_id = $1 ORDER BY capability",
    )
    .bind(project_id)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows.into_iter().map(|r| r.get("capability")).collect())
}

/// Whether any Project currently holds `registration.gatekeeper`. This is the
/// shared-secret kill switch: once a real gatekeeper is installed, the
/// bootstrap shared-secret registration path auto-disables (docs/24).
pub async fn any_gatekeeper_exists(conn: &mut PgConnection) -> Result<bool, sqlx::Error> {
    let row = sqlx::query("SELECT 1 AS ok FROM project_capabilities WHERE capability = $1 LIMIT 1")
        .bind(REGISTRATION_GATEKEEPER)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(row.is_some())
}

/// Whether a Project (by id) holds a capability via an explicit grant. Does
/// NOT apply the superuser short-circuit — use [`account_has_capability`] for
/// the account-level check.
pub async fn project_has(
    conn: &mut PgConnection,
    project_id: i64,
    capability: &str,
) -> Result<bool, sqlx::Error> {
    let row = sqlx::query(
        "SELECT 1 AS ok FROM project_capabilities WHERE project_id = $1 AND capability = $2",
    )
    .bind(project_id)
    .bind(capability)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.is_some())
}

/// Whether a *bot account* effectively holds a capability: true if its Project
/// is the pinned adminbot Project (superuser), otherwise true iff its Project
/// has an explicit grant. Accounts not linked to any Project hold nothing.
pub async fn account_has_capability(
    conn: &mut PgConnection,
    account_id: i64,
    capability: &str,
) -> Result<bool, sqlx::Error> {
    let Some((project_id, slug)) = crate::db::projects::project_for_account(conn, account_id).await?
    else {
        return Ok(false);
    };
    if slug == ADMINBOT_PROJECT_SLUG {
        return Ok(true); // superuser pin: all capabilities
    }
    project_has(conn, project_id, capability).await
}
