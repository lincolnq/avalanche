//! OAuth grant storage for Project login ("Sign in with Avalanche",
//! docs/25-project-login.md).
//!
//! Backs both OAuth front-ends behind one table (see
//! `022_oauth_grants.sql`): same-device Authorization Code + PKCE
//! (`auth_code`) and cross-device Device Authorization Grant (`device_code`).
//! The minted access token is a `project_tokens` row, so `verify` is unchanged.

use sqlx::{PgConnection, Row};
use time::OffsetDateTime;

/// A row of `oauth_grants`, as read back for validation/exchange.
#[derive(Debug, Clone)]
pub struct OauthGrant {
    pub code: String,
    pub grant_type: String,
    pub user_code: Option<String>,
    pub client_id: String,
    pub project_url: String,
    pub redirect_uri: Option<String>,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
    pub scope: Option<String>,
    pub account_id: Option<i64>,
    pub status: String,
    pub access_token: Option<String>,
    pub auth_time: Option<OffsetDateTime>,
    pub expires_at: OffsetDateTime,
    pub last_polled_at: Option<OffsetDateTime>,
}

fn row_to_grant(r: &sqlx::postgres::PgRow) -> OauthGrant {
    OauthGrant {
        code: r.get("code"),
        grant_type: r.get("grant_type"),
        user_code: r.get("user_code"),
        client_id: r.get("client_id"),
        project_url: r.get("project_url"),
        redirect_uri: r.get("redirect_uri"),
        code_challenge: r.get("code_challenge"),
        code_challenge_method: r.get("code_challenge_method"),
        scope: r.get("scope"),
        account_id: r.get("account_id"),
        status: r.get("status"),
        access_token: r.get("access_token"),
        auth_time: r.get("auth_time"),
        expires_at: r.get("expires_at"),
        last_polled_at: r.get("last_polled_at"),
    }
}

const SELECT_COLS: &str = "code, grant_type, user_code, client_id, project_url, \
     redirect_uri, code_challenge, code_challenge_method, scope, account_id, \
     status, access_token, auth_time, expires_at, last_polled_at";

/// Create a same-device authorization code, already bound to the consenting
/// account and carrying the PKCE challenge. Status is `approved` (the user has
/// consented in-app); it becomes `consumed` at token exchange (single-use).
/// `auth_time` is set to now (the moment of consent).
#[allow(clippy::too_many_arguments)]
pub async fn create_auth_code(
    conn: &mut PgConnection,
    code: &str,
    account_id: i64,
    client_id: &str,
    project_url: &str,
    redirect_uri: &str,
    code_challenge: &str,
    code_challenge_method: &str,
    scope: Option<&str>,
    lifetime_secs: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO oauth_grants
           (code, grant_type, client_id, project_url, redirect_uri,
            code_challenge, code_challenge_method, scope, account_id,
            status, auth_time, expires_at)
         VALUES ($1, 'auth_code', $2, $3, $4, $5, $6, $7, $8,
                 'approved', now(), now() + make_interval(secs => $9))",
    )
    .bind(code)
    .bind(client_id)
    .bind(project_url)
    .bind(redirect_uri)
    .bind(code_challenge)
    .bind(code_challenge_method)
    .bind(scope)
    .bind(account_id)
    .bind(lifetime_secs as f64)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Create a cross-device device-authorization grant, `pending` with no account.
/// Returns `expires_at`.
pub async fn create_device(
    conn: &mut PgConnection,
    code: &str,
    user_code: &str,
    client_id: &str,
    project_url: &str,
    scope: Option<&str>,
    lifetime_secs: i64,
) -> Result<OffsetDateTime, sqlx::Error> {
    let row = sqlx::query(
        "INSERT INTO oauth_grants
           (code, grant_type, user_code, client_id, project_url, scope,
            status, expires_at)
         VALUES ($1, 'device_code', $2, $3, $4, $5, 'pending',
                 now() + make_interval(secs => $6))
         RETURNING expires_at",
    )
    .bind(code)
    .bind(user_code)
    .bind(client_id)
    .bind(project_url)
    .bind(scope)
    .bind(lifetime_secs as f64)
    .fetch_one(&mut *conn)
    .await?;
    Ok(row.get("expires_at"))
}

/// Fetch a grant by its opaque `code` (device_code or auth code). Does not
/// filter on expiry or status — the caller decides how to interpret them.
pub async fn get(conn: &mut PgConnection, code: &str) -> Result<Option<OauthGrant>, sqlx::Error> {
    let row = sqlx::query(&format!(
        "SELECT {SELECT_COLS} FROM oauth_grants WHERE code = $1"
    ))
    .bind(code)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.as_ref().map(row_to_grant))
}

/// Look up a `pending`, unexpired device grant by its short `user_code`.
pub async fn find_pending_device_by_user_code(
    conn: &mut PgConnection,
    user_code: &str,
) -> Result<Option<OauthGrant>, sqlx::Error> {
    let row = sqlx::query(&format!(
        "SELECT {SELECT_COLS} FROM oauth_grants
         WHERE user_code = $1 AND grant_type = 'device_code'
           AND status = 'pending' AND expires_at > now()"
    ))
    .bind(user_code)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.as_ref().map(row_to_grant))
}

/// Approve a pending device grant: bind the account, store the minted access
/// token, stamp `auth_time`. Returns the number of rows updated (0 if the grant
/// was not `pending` — e.g. already approved or expired-swept).
pub async fn approve_device(
    conn: &mut PgConnection,
    code: &str,
    account_id: i64,
    access_token: &str,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE oauth_grants
         SET account_id = $2, access_token = $3, status = 'approved', auth_time = now()
         WHERE code = $1 AND status = 'pending'",
    )
    .bind(code)
    .bind(account_id)
    .bind(access_token)
    .execute(&mut *conn)
    .await?;
    Ok(result.rows_affected())
}

/// Record a device-grant poll (sets `last_polled_at = now()`). Used with the
/// prior value from [`get`] to enforce the RFC 8628 `slow_down` interval.
pub async fn mark_polled(conn: &mut PgConnection, code: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE oauth_grants SET last_polled_at = now() WHERE code = $1")
        .bind(code)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Mark an `approved` grant `consumed` (single-use). Returns rows affected — 0
/// if it was not in `approved` state (already consumed, or never approved).
pub async fn consume(conn: &mut PgConnection, code: &str) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE oauth_grants SET status = 'consumed' WHERE code = $1 AND status = 'approved'",
    )
    .bind(code)
    .execute(&mut *conn)
    .await?;
    Ok(result.rows_affected())
}

/// Delete expired grants. Runs on the background GC interval.
pub async fn delete_expired(conn: &mut PgConnection) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM oauth_grants WHERE expires_at < now()")
        .execute(&mut *conn)
        .await?;
    Ok(result.rows_affected())
}
