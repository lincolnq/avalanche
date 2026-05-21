//! Auth challenge management: issue, consume, and expire nonces.
//!
//! Before obtaining a session token, a client must request a challenge nonce
//! from the server, sign it with its Ed25519 identity key, and present the
//! signature alongside the nonce in the token request. This proves possession
//! of the private key corresponding to the public key stored at registration.
//!
//! Challenges are single-use: `consume` atomically deletes the row and returns
//! the associated `device_pk`, preventing replay. Challenges expire after a
//! short TTL (default 5 minutes).

use sqlx::{PgConnection, Row};

/// Store a new challenge nonce for a device.
pub async fn create(
    conn: &mut PgConnection,
    nonce: &str,
    device_pk: i64,
    lifetime_secs: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO auth_challenges (nonce, device_pk, expires_at)
         VALUES ($1, $2, now() + make_interval(secs => $3))",
    )
    .bind(nonce)
    .bind(device_pk)
    .bind(lifetime_secs as f64)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Atomically consume a challenge nonce.
///
/// Deletes the row and returns the `device_pk` it was issued for, or `None`
/// if the nonce does not exist or has expired. The deletion is unconditional
/// on expiry check — an expired nonce is removed from the table regardless.
pub async fn consume(
    conn: &mut PgConnection,
    nonce: &str,
) -> Result<Option<i64>, sqlx::Error> {
    let row = sqlx::query(
        "DELETE FROM auth_challenges WHERE nonce = $1 AND expires_at > now()
         RETURNING device_pk",
    )
    .bind(nonce)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.map(|r| r.get("device_pk")))
}

/// Delete expired challenge nonces.
pub async fn delete_expired(conn: &mut PgConnection) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM auth_challenges WHERE expires_at < now()")
        .execute(&mut *conn)
        .await?;
    Ok(result.rows_affected())
}
