//! Abuse reports (docs/12 §3).
//!
//! Account-level signals only — no message content ever reaches the server.
//! Reports are persisted for operator review; the enforcement ladder (§4) and
//! cross-server signed forwarding (§3) are deferred until federation lands.

use sqlx::{PgConnection, Row};

/// The reason enum carried by a report (docs/12 §3 "What is reported").
pub const REASONS: &[&str] = &["spam", "harassment", "impersonation", "other"];

/// Whether `reason` is one of the accepted [`REASONS`].
pub fn is_valid_reason(reason: &str) -> bool {
    REASONS.contains(&reason)
}

pub struct AbuseReport {
    pub id: i64,
    pub reported_did: String,
    pub reason: String,
    pub reporter_account: i64,
}

/// Persist a report filed by `reporter_account` against `reported_did`.
/// Returns the new report id.
pub async fn insert(
    conn: &mut PgConnection,
    reported_did: &str,
    reason: &str,
    reporter_account: i64,
) -> Result<i64, sqlx::Error> {
    let row = sqlx::query(
        "INSERT INTO abuse_reports (reported_did, reason, reporter_account)
         VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(reported_did)
    .bind(reason)
    .bind(reporter_account)
    .fetch_one(&mut *conn)
    .await?;
    Ok(row.get("id"))
}

/// Count the reports an account has filed (per-reporter audit / tests).
pub async fn count_by_reporter(
    conn: &mut PgConnection,
    reporter_account: i64,
) -> Result<i64, sqlx::Error> {
    let row = sqlx::query("SELECT count(*) AS n FROM abuse_reports WHERE reporter_account = $1")
        .bind(reporter_account)
        .fetch_one(&mut *conn)
        .await?;
    Ok(row.get("n"))
}

/// List reports against a DID, newest first — the operator-review surface.
pub async fn list_for_did(
    conn: &mut PgConnection,
    reported_did: &str,
) -> Result<Vec<AbuseReport>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, reported_did, reason, reporter_account FROM abuse_reports
         WHERE reported_did = $1 ORDER BY id DESC",
    )
    .bind(reported_did)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| AbuseReport {
            id: r.get("id"),
            reported_did: r.get("reported_did"),
            reason: r.get("reason"),
            reporter_account: r.get("reporter_account"),
        })
        .collect())
}
