//! Group-message store-and-forward queue (sealed-sender path).
//!
//! Mirrors `db::messages` but keyed by `recipient_group_pseudonym` rather
//! than `device_pk`. The pseudonym is opaque to the server: it's chosen
//! by the recipient at group-join time and looked up via the sealed-
//! sender envelope's recipient list. The server therefore can't link a
//! queued group message back to a DID or device.
//!
//! Lifecycle:
//! - `enqueue` writes one row per recipient.
//! - `fetch_for_pseudonym` returns oldest-first for a single pseudonym
//!   when the device drains pending messages on (re)connect.
//! - `acknowledge` is scoped to the pseudonym; a client cannot delete
//!   another pseudonym's messages.
//! - `delete_expired` is the background sweeper.

use sqlx::{PgConnection, Row};
use time::OffsetDateTime;

pub struct QueuedGroupMessage {
    pub id: i64,
    pub group_id: Vec<u8>,
    pub ciphertext: Vec<u8>,
    pub enqueued_at: OffsetDateTime,
}

pub async fn enqueue(
    conn: &mut PgConnection,
    recipient_group_pseudonym: &[u8],
    group_id: &[u8],
    ciphertext: &[u8],
    expiry_secs: i64,
) -> Result<i64, sqlx::Error> {
    let row = sqlx::query(
        "INSERT INTO group_message_queue
         (recipient_group_pseudonym, group_id, ciphertext, expires_at)
         VALUES ($1, $2, $3, now() + make_interval(secs => $4))
         RETURNING id",
    )
    .bind(recipient_group_pseudonym)
    .bind(group_id)
    .bind(ciphertext)
    .bind(expiry_secs as f64)
    .fetch_one(&mut *conn)
    .await?;
    Ok(row.get("id"))
}

pub async fn fetch_for_pseudonym(
    conn: &mut PgConnection,
    recipient_group_pseudonym: &[u8],
) -> Result<Vec<QueuedGroupMessage>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, group_id, ciphertext, enqueued_at
         FROM group_message_queue
         WHERE recipient_group_pseudonym = $1
         ORDER BY enqueued_at ASC, id ASC",
    )
    .bind(recipient_group_pseudonym)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| QueuedGroupMessage {
            id: r.get("id"),
            group_id: r.get("group_id"),
            ciphertext: r.get("ciphertext"),
            enqueued_at: r.get("enqueued_at"),
        })
        .collect())
}

pub async fn acknowledge(
    conn: &mut PgConnection,
    recipient_group_pseudonym: &[u8],
    message_ids: &[i64],
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "DELETE FROM group_message_queue
         WHERE recipient_group_pseudonym = $1 AND id = ANY($2)",
    )
    .bind(recipient_group_pseudonym)
    .bind(message_ids)
    .execute(&mut *conn)
    .await?;
    Ok(result.rows_affected())
}

pub async fn delete_expired(conn: &mut PgConnection) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM group_message_queue WHERE expires_at < now()")
        .execute(&mut *conn)
        .await?;
    Ok(result.rows_affected())
}
