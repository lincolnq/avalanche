//! Project token management: issue, verify, and expire.
//!
//! Project tokens are opaque 256-bit random strings that let Projects verify
//! a user's identity. They are short-lived (1 hour) and scoped to a specific
//! Project URL. The token is issued by the homeserver at the user's request
//! and verified by the Project via a single HTTP call.

use sqlx::{PgConnection, Row};
use time::OffsetDateTime;

/// Issue a new project token for an account.
pub async fn create(
    conn: &mut PgConnection,
    token: &str,
    account_id: i64,
    project_url: &str,
    lifetime_secs: i64,
) -> Result<OffsetDateTime, sqlx::Error> {
    let row = sqlx::query(
        "INSERT INTO project_tokens (token, account_id, project_url, expires_at)
         VALUES ($1, $2, $3, now() + make_interval(secs => $4))
         RETURNING expires_at",
    )
    .bind(token)
    .bind(account_id)
    .bind(project_url)
    .bind(lifetime_secs as f64)
    .fetch_one(&mut *conn)
    .await?;
    Ok(row.get("expires_at"))
}

/// Verify a project token. Returns (DID, project_url) if valid and not expired.
pub async fn verify(
    conn: &mut PgConnection,
    token: &str,
) -> Result<Option<(String, String)>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT a.did, pt.project_url
         FROM project_tokens pt
         JOIN accounts a ON a.id = pt.account_id
         WHERE pt.token = $1 AND pt.expires_at > now()",
    )
    .bind(token)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.map(|r| (r.get("did"), r.get("project_url"))))
}

/// Delete expired project tokens.
pub async fn delete_expired(conn: &mut PgConnection) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM project_tokens WHERE expires_at < now()")
        .execute(&mut *conn)
        .await?;
    Ok(result.rows_affected())
}
