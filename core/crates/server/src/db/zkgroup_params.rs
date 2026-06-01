//! Storage of the homeserver's group-crypto bundle.
//!
//! Exactly one active version is expected at a time. The [`load_or_init`]
//! flow upserts the current version atomically on first boot and returns
//! the persisted bytes on subsequent boots, so concurrent server startups
//! (or migrate-then-start sequences) all converge on the same key.

use sqlx::{PgConnection, Row};

/// Active bundle version. History:
/// - v1: serialized `ServerSecretParams` (custom DID-shaped credentials).
/// - v2: serialized `ServerSecretParams` (stock zkgroup, post §2.3 option 1).
/// - v3: bincoded [`GroupCryptoBundle`] — `ServerSecretParams` *plus*
///   sender-cert chain bytes ([`crypto::sender_cert::SenderCertChain`]).
///   Bumped when sealed-sender group send landed; rows from earlier
///   versions remain in the table but are never read.
pub const CURRENT_VERSION: i32 = 3;

/// Bundle persisted alongside the homeserver's zkgroup signing key. Holds
/// everything the server needs to authorize group operations: zkgroup
/// `ServerSecretParams` (credential / endorsement issuance) and the
/// `SenderCertChain` (signing per-message sender certificates for the
/// sealed-sender flow).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct GroupCryptoBundle {
    pub zkgroup_secret: Vec<u8>,
    pub sender_cert_chain: Vec<u8>,
}

impl GroupCryptoBundle {
    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).expect("serialize GroupCryptoBundle")
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(bytes)
    }
}

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
