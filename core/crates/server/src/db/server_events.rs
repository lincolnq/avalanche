//! Durable server-event log for bot catch-up (docs/22 join-event API).
//!
//! Append-only. Live pushes go out over the WebSocket to capability-holding
//! bots; this table lets a bot that was disconnected recover missed events via
//! `GET /v1/admin/events?since=<id>`. No group linkage is stored, preserving
//! the §3.9 membership-opacity discipline.

use sqlx::{PgConnection, Row};

pub const KIND_ACCOUNT_JOINED: &str = "account_joined";

pub struct ServerEvent {
    pub id: i64,
    pub kind: String,
    pub did: String,
    pub invite_token: Option<String>,
    pub joined_at_ms: i64,
}

/// Append an `account_joined` event. Returns the new event id.
pub async fn append_account_joined(
    conn: &mut PgConnection,
    did: &str,
    invite_token: Option<&str>,
    joined_at_ms: i64,
) -> Result<i64, sqlx::Error> {
    let row = sqlx::query(
        "INSERT INTO server_events (kind, did, invite_token, joined_at_ms)
         VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(KIND_ACCOUNT_JOINED)
    .bind(did)
    .bind(invite_token)
    .bind(joined_at_ms)
    .fetch_one(&mut *conn)
    .await?;
    Ok(row.get("id"))
}

/// Fetch events of a kind with id greater than `since`, oldest first, capped
/// at `limit`.
pub async fn fetch_since(
    conn: &mut PgConnection,
    since: i64,
    kind: &str,
    limit: i64,
) -> Result<Vec<ServerEvent>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, kind, did, invite_token, joined_at_ms FROM server_events
         WHERE kind = $1 AND id > $2 ORDER BY id ASC LIMIT $3",
    )
    .bind(kind)
    .bind(since)
    .bind(limit)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| ServerEvent {
            id: r.get("id"),
            kind: r.get("kind"),
            did: r.get("did"),
            invite_token: r.get("invite_token"),
            joined_at_ms: r.get("joined_at_ms"),
        })
        .collect())
}

/// Delete events older than `max_age_secs`. Returns the number removed.
pub async fn delete_older_than(
    conn: &mut PgConnection,
    max_age_secs: i64,
) -> Result<u64, sqlx::Error> {
    let res = sqlx::query(
        "DELETE FROM server_events WHERE created_at < now() - make_interval(secs => $1)",
    )
    .bind(max_age_secs as f64)
    .execute(&mut *conn)
    .await?;
    Ok(res.rows_affected())
}
