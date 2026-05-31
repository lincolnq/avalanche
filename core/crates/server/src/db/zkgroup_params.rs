//! Storage of the homeserver's zkgroup signing key.
//!
//! Exactly one active version is expected during Stage 5. The
//! [`load_or_init`] flow upserts version 1 atomically on first boot and
//! returns the persisted bytes on subsequent boots, so concurrent server
//! startups (or migrate-then-start sequences) all converge on the same key.

use sqlx::{PgConnection, Row};

/// The version pinned for Stage 5. Schema allows multiple versions for
/// future key rotation. Bumped to 2 when the wire format changed during
/// the §2.3-option-2 → §2.3-option-1 migration (see docs/03-groups.md
/// §2.4): old bytes used a bundled `CredentialKeyPair` field that the
/// new `ServerSecretParams` doesn't carry. Existing version-1 rows
/// remain in the table but are never read.
pub const CURRENT_VERSION: i32 = 2;

/// Load the active version's serialized `ServerSecretParams`, or insert and
/// return new params produced by `generate` if none exist yet.
///
/// `generate` is a closure rather than a value so we don't run a CSPRNG on
/// every boot — only on first boot, before the row exists.
pub async fn load_or_init<F>(
    conn: &mut PgConnection,
    version: i32,
    generate: F,
) -> Result<Vec<u8>, sqlx::Error>
where
    F: FnOnce() -> Vec<u8>,
{
    // First try the cheap read.
    if let Some(row) = sqlx::query("SELECT params FROM zkgroup_server_params WHERE version = $1")
        .bind(version)
        .fetch_optional(&mut *conn)
        .await?
    {
        return Ok(row.get::<Vec<u8>, _>("params"));
    }

    // Not present yet — try to insert. If another concurrent process inserted
    // between our SELECT and INSERT, ON CONFLICT DO NOTHING + re-SELECT lets
    // us converge on whichever bytes won.
    let new_params = generate();
    sqlx::query(
        "INSERT INTO zkgroup_server_params (version, params)
         VALUES ($1, $2)
         ON CONFLICT (version) DO NOTHING",
    )
    .bind(version)
    .bind(&new_params)
    .execute(&mut *conn)
    .await?;

    let row = sqlx::query("SELECT params FROM zkgroup_server_params WHERE version = $1")
        .bind(version)
        .fetch_one(&mut *conn)
        .await?;
    Ok(row.get::<Vec<u8>, _>("params"))
}
