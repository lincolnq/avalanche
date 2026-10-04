//! Per-Project bot signup keys (docs/24, docs/09 S-01).
//!
//! A bot signup key admits a bot account on a closed server and links it to
//! the Project that owns the key — nothing else. One active key per Project;
//! minting again replaces it (revoking the old one). Only the SHA-256 of the
//! key is stored, and the owning Project is resolved from that hash, so a key
//! can never choose which Project it links into.

use sha2::{Digest, Sha256};
use sqlx::PgConnection;

/// Store (or replace) the hash of a Project's bot signup key.
pub async fn set(
    conn: &mut PgConnection,
    project_id: i64,
    key: &[u8],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO project_bot_signup_keys (project_id, key_hash) VALUES ($1, $2)
         ON CONFLICT (project_id) DO UPDATE SET key_hash = EXCLUDED.key_hash, created_at = now()",
    )
    .bind(project_id)
    .bind(Sha256::digest(key).as_slice())
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Resolve the Project that owns a presented bot signup key. `None` if the key
/// is unknown (never minted, or replaced by a newer one).
pub async fn project_for_key(
    conn: &mut PgConnection,
    key: &[u8],
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar("SELECT project_id FROM project_bot_signup_keys WHERE key_hash = $1")
        .bind(Sha256::digest(key).as_slice())
        .fetch_optional(&mut *conn)
        .await
}
