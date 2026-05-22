//! Prekey storage, upload, and atomic consumption.
//!
//! Signal's X3DH protocol requires each device to publish prekey bundles so
//! other users can initiate encrypted sessions without the recipient being
//! online. This module manages three kinds of server-side prekeys:
//!
//! - **Signed EC prekeys** — medium-term keys rotated periodically.
//! - **One-time EC prekeys** — ephemeral keys consumed once per session.
//! - **Kyber prekeys** — post-quantum keys for harvest-now-decrypt-later
//!   resistance.
//!
//! # Security notes
//!
//! - All prekey columns store **public key material only**. Private halves
//!   never leave the client.
//! - One-time prekey consumption uses `DELETE ... RETURNING` in a single SQL
//!   statement to ensure atomicity under concurrent requests. Two callers
//!   fetching the same device's bundle will each consume a different key.
//! - If the one-time pool is empty, the bundle is returned without one. X3DH
//!   still works but the first message has weaker forward secrecy.
//! - The server cannot forge prekeys (they are signed by the device's
//!   identity key), but a compromised server could serve stale prekeys or
//!   withhold them. Key transparency (deferred) would address this.

use sqlx::{PgConnection, Row};

pub struct SignedPreKey {
    pub id: i32,
    pub public_key: Vec<u8>,
    pub signature: Vec<u8>,
}

pub struct OneTimePreKey {
    pub id: i32,
    pub public_key: Vec<u8>,
}

pub struct KyberPreKey {
    pub id: i32,
    pub public_key: Vec<u8>,
    pub signature: Vec<u8>,
}

pub struct PreKeyBundle {
    pub identity_key: Vec<u8>,
    pub registration_id: i32,
    pub signed_prekey: SignedPreKey,
    pub one_time_prekey: Option<OneTimePreKey>,
    pub kyber_prekey: KyberPreKey,
}

/// Upload a signed prekey (replaces any existing for this device).
pub async fn upsert_signed(
    conn: &mut PgConnection,
    device_pk: i64,
    id: i32,
    public_key: &[u8],
    signature: &[u8],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO signed_prekeys (id, device_pk, public_key, signature)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (device_pk, id) DO UPDATE
         SET public_key = $3, signature = $4, uploaded_at = now()",
    )
    .bind(id)
    .bind(device_pk)
    .bind(public_key)
    .bind(signature)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Upload a batch of one-time prekeys.
pub async fn insert_one_time_batch(
    conn: &mut PgConnection,
    device_pk: i64,
    keys: &[(i32, Vec<u8>)],
) -> Result<(), sqlx::Error> {
    for (id, public_key) in keys {
        sqlx::query(
            "INSERT INTO one_time_prekeys (id, device_pk, public_key)
             VALUES ($1, $2, $3)
             ON CONFLICT (device_pk, id) DO NOTHING",
        )
        .bind(id)
        .bind(device_pk)
        .bind(public_key.as_slice())
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// Upload a batch of one-time Kyber prekeys.
pub async fn insert_one_time_kyber_batch(
    conn: &mut PgConnection,
    device_pk: i64,
    keys: &[(i32, Vec<u8>, Vec<u8>)],
) -> Result<(), sqlx::Error> {
    for (id, public_key, signature) in keys {
        sqlx::query(
            "INSERT INTO one_time_kyber_prekeys (id, device_pk, public_key, signature)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (device_pk, id) DO NOTHING",
        )
        .bind(id)
        .bind(device_pk)
        .bind(public_key.as_slice())
        .bind(signature.as_slice())
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// Upload a Kyber prekey (replaces any existing for this device).
pub async fn upsert_kyber(
    conn: &mut PgConnection,
    device_pk: i64,
    id: i32,
    public_key: &[u8],
    signature: &[u8],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO kyber_prekeys (id, device_pk, public_key, signature)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (device_pk, id) DO UPDATE
         SET public_key = $3, signature = $4, uploaded_at = now()",
    )
    .bind(id)
    .bind(device_pk)
    .bind(public_key)
    .bind(signature)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Fetch a prekey bundle for a device. Atomically consumes one one-time prekey.
pub async fn fetch_bundle(
    conn: &mut PgConnection,
    device_pk: i64,
) -> Result<Option<PreKeyBundle>, sqlx::Error> {
    let device = sqlx::query(
        "SELECT identity_key, registration_id FROM devices WHERE id = $1",
    )
    .bind(device_pk)
    .fetch_optional(&mut *conn)
    .await?;

    let device = match device {
        Some(d) => d,
        None => return Ok(None),
    };

    let signed = sqlx::query(
        "SELECT id, public_key, signature FROM signed_prekeys
         WHERE device_pk = $1 ORDER BY id DESC LIMIT 1",
    )
    .bind(device_pk)
    .fetch_optional(&mut *conn)
    .await?;

    let signed = match signed {
        Some(r) => SignedPreKey {
            id: r.get("id"),
            public_key: r.get("public_key"),
            signature: r.get("signature"),
        },
        None => return Ok(None),
    };

    // Atomically consume one one-time prekey.
    let one_time = sqlx::query(
        "DELETE FROM one_time_prekeys
         WHERE device_pk = $1
           AND id = (SELECT id FROM one_time_prekeys WHERE device_pk = $1 ORDER BY id LIMIT 1)
         RETURNING id, public_key",
    )
    .bind(device_pk)
    .fetch_optional(&mut *conn)
    .await?
    .map(|r| OneTimePreKey {
        id: r.get("id"),
        public_key: r.get("public_key"),
    });

    // Atomically consume one one-time Kyber prekey; fall back to last-resort if pool is empty.
    let kyber = sqlx::query(
        "DELETE FROM one_time_kyber_prekeys
         WHERE device_pk = $1
           AND id = (SELECT id FROM one_time_kyber_prekeys WHERE device_pk = $1 ORDER BY id LIMIT 1)
         RETURNING id, public_key, signature",
    )
    .bind(device_pk)
    .fetch_optional(&mut *conn)
    .await?
    .map(|r| KyberPreKey {
        id: r.get("id"),
        public_key: r.get("public_key"),
        signature: r.get("signature"),
    });

    let kyber = match kyber {
        Some(k) => k,
        None => {
            // Pool empty — fall back to last-resort Kyber prekey.
            let row = sqlx::query(
                "SELECT id, public_key, signature FROM kyber_prekeys
                 WHERE device_pk = $1 ORDER BY id DESC LIMIT 1",
            )
            .bind(device_pk)
            .fetch_optional(&mut *conn)
            .await?;

            match row {
                Some(r) => KyberPreKey {
                    id: r.get("id"),
                    public_key: r.get("public_key"),
                    signature: r.get("signature"),
                },
                None => return Ok(None),
            }
        }
    };

    Ok(Some(PreKeyBundle {
        identity_key: device.get("identity_key"),
        registration_id: device.get("registration_id"),
        signed_prekey: signed,
        one_time_prekey: one_time,
        kyber_prekey: kyber,
    }))
}

/// Count remaining one-time prekeys for a device.
pub async fn one_time_count(conn: &mut PgConnection, device_pk: i64) -> Result<i64, sqlx::Error> {
    let row = sqlx::query("SELECT COUNT(*) as count FROM one_time_prekeys WHERE device_pk = $1")
        .bind(device_pk)
        .fetch_one(&mut *conn)
        .await?;
    Ok(row.get::<i64, _>("count"))
}

/// Count remaining one-time Kyber prekeys for a device.
pub async fn one_time_kyber_count(
    conn: &mut PgConnection,
    device_pk: i64,
) -> Result<i64, sqlx::Error> {
    let row = sqlx::query(
        "SELECT COUNT(*) as count FROM one_time_kyber_prekeys WHERE device_pk = $1",
    )
    .bind(device_pk)
    .fetch_one(&mut *conn)
    .await?;
    Ok(row.get::<i64, _>("count"))
}

/// Count remaining Kyber prekeys for a device.
pub async fn kyber_count(conn: &mut PgConnection, device_pk: i64) -> Result<i64, sqlx::Error> {
    let row = sqlx::query("SELECT COUNT(*) as count FROM kyber_prekeys WHERE device_pk = $1")
        .bind(device_pk)
        .fetch_one(&mut *conn)
        .await?;
    Ok(row.get::<i64, _>("count"))
}
