//! Single-use tracking for Project-signed registration tokens (docs/24).
//!
//! Each token carries a unique `jti`. Redemption is the PRIMARY KEY: we INSERT
//! as the gate (before account creation), so a replayed token conflicts and is
//! rejected. A token is consumed even if a later registration step fails — the
//! safe (fail-closed) direction.
//!
//! Generic by design (`purpose` column) so a future bot-signup token
//! (`purpose = "bot"`) shares this table with the human-invite token
//! (`purpose = "invite"`).

use sqlx::PgConnection;

/// Atomically redeem a token id. Returns `true` if this call consumed it,
/// `false` if it was already redeemed (replay).
pub async fn try_redeem(
    conn: &mut PgConnection,
    jti: &str,
    issuer_slug: &str,
    purpose: &str,
    redeemed_by_did: &str,
) -> Result<bool, sqlx::Error> {
    let res = sqlx::query(
        "INSERT INTO token_redemptions (jti, issuer_slug, purpose, redeemed_by_did)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (jti) DO NOTHING",
    )
    .bind(jti)
    .bind(issuer_slug)
    .bind(purpose)
    .bind(redeemed_by_did)
    .execute(&mut *conn)
    .await?;
    Ok(res.rows_affected() == 1)
}

/// Whether a token id has already been redeemed.
pub async fn is_redeemed(conn: &mut PgConnection, jti: &str) -> Result<bool, sqlx::Error> {
    let row = sqlx::query("SELECT 1 AS ok FROM token_redemptions WHERE jti = $1")
        .bind(jti)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(row.is_some())
}
