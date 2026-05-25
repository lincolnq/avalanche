//! Device registration and lookup.
//!
//! Each account may have multiple devices (phone, tablet, desktop). Every
//! device has its own identity key, prekey bundles, and Double Ratchet
//! sessions. The server stores the public identity key so it can be included
//! in prekey bundles served to other users.
//!
//! The internal `id` (bigint) is the foreign key used by session tokens,
//! prekey tables, and the message queue. The external identifier is the
//! `(account DID, device_id)` pair.
//!
//! # Security note
//!
//! The `identity_key` column stores only the **public** half of the device's
//! identity key. The private half never leaves the client device.

use sqlx::{PgConnection, Row};

pub struct Device {
    pub id: i64,
    pub account_id: i64,
    pub device_id: i32,
    pub identity_key: Vec<u8>,
    pub registration_id: i32,
}

impl Device {
    fn from_row(row: sqlx::postgres::PgRow) -> Self {
        Self {
            id: row.get("id"),
            account_id: row.get("account_id"),
            device_id: row.get("device_id"),
            identity_key: row.get("identity_key"),
            registration_id: row.get("registration_id"),
        }
    }
}

/// Register a new device for an account.
pub async fn create(
    conn: &mut PgConnection,
    account_id: i64,
    device_id: i32,
    identity_key: &[u8],
    registration_id: i32,
) -> Result<i64, sqlx::Error> {
    let row = sqlx::query(
        "INSERT INTO devices (account_id, device_id, identity_key, registration_id)
         VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(account_id)
    .bind(device_id)
    .bind(identity_key)
    .bind(registration_id)
    .fetch_one(&mut *conn)
    .await?;
    Ok(row.get("id"))
}

/// Look up a device by account internal ID and device_id.
pub async fn find(
    conn: &mut PgConnection,
    account_id: i64,
    device_id: i32,
) -> Result<Option<Device>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, account_id, device_id, identity_key, registration_id
         FROM devices WHERE account_id = $1 AND device_id = $2",
    )
    .bind(account_id)
    .bind(device_id)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.map(Device::from_row))
}

/// Look up a device by DID and device_id (joins through accounts).
pub async fn find_by_did(
    conn: &mut PgConnection,
    did: &str,
    device_id: i32,
) -> Result<Option<Device>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT d.id, d.account_id, d.device_id, d.identity_key, d.registration_id
         FROM devices d
         JOIN accounts a ON a.id = d.account_id
         WHERE a.did = $1 AND d.device_id = $2",
    )
    .bind(did)
    .bind(device_id)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.map(Device::from_row))
}

/// Look up a device by its internal PK.
pub async fn find_by_pk(conn: &mut PgConnection, device_pk: i64) -> Result<Option<Device>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, account_id, device_id, identity_key, registration_id
         FROM devices WHERE id = $1",
    )
    .bind(device_pk)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.map(Device::from_row))
}

/// Delete a device and all its dependent rows (session tokens, prekeys, queued messages).
pub async fn delete(conn: &mut PgConnection, device_pk: i64) -> Result<(), sqlx::Error> {
    // Delete dependent rows first (no ON DELETE CASCADE in schema).
    sqlx::query("DELETE FROM session_tokens WHERE device_pk = $1")
        .bind(device_pk)
        .execute(&mut *conn)
        .await?;
    sqlx::query("DELETE FROM auth_challenges WHERE device_pk = $1")
        .bind(device_pk)
        .execute(&mut *conn)
        .await?;
    sqlx::query("DELETE FROM signed_prekeys WHERE device_pk = $1")
        .bind(device_pk)
        .execute(&mut *conn)
        .await?;
    sqlx::query("DELETE FROM one_time_prekeys WHERE device_pk = $1")
        .bind(device_pk)
        .execute(&mut *conn)
        .await?;
    sqlx::query("DELETE FROM kyber_prekeys WHERE device_pk = $1")
        .bind(device_pk)
        .execute(&mut *conn)
        .await?;
    sqlx::query("DELETE FROM one_time_kyber_prekeys WHERE device_pk = $1")
        .bind(device_pk)
        .execute(&mut *conn)
        .await?;
    sqlx::query("DELETE FROM message_queue WHERE recipient_device_pk = $1")
        .bind(device_pk)
        .execute(&mut *conn)
        .await?;
    sqlx::query("DELETE FROM push_pseudonyms WHERE device_pk = $1")
        .bind(device_pk)
        .execute(&mut *conn)
        .await?;
    sqlx::query("DELETE FROM devices WHERE id = $1")
        .bind(device_pk)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// List all devices for an account (by DID).
pub async fn list_by_did(conn: &mut PgConnection, did: &str) -> Result<Vec<Device>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT d.id, d.account_id, d.device_id, d.identity_key, d.registration_id
         FROM devices d
         JOIN accounts a ON a.id = d.account_id
         WHERE a.did = $1
         ORDER BY d.device_id",
    )
    .bind(did)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows.into_iter().map(Device::from_row).collect())
}
