//! Account storage: create and look up accounts by DID.
//!
//! An account represents a user identity on this homeserver. The external
//! identifier is a `did:plc:...` string; the internal `id` (bigint) is used
//! as a foreign key throughout the schema but never exposed in the API.

use sqlx::{Acquire, PgConnection, Row};

pub struct Account {
    pub id: i64,
    pub did: String,
    pub display_name: Option<String>,
    pub is_bot: bool,
    pub recovery_blob: Option<Vec<u8>>,
}

/// Create a new account and return its internal ID.
pub async fn create(
    conn: &mut PgConnection,
    did: &str,
    display_name: Option<&str>,
    is_bot: bool,
) -> Result<i64, sqlx::Error> {
    let row = sqlx::query(
        "INSERT INTO accounts (did, display_name, is_bot) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(did)
    .bind(display_name)
    .bind(is_bot)
    .fetch_one(&mut *conn)
    .await?;
    Ok(row.get("id"))
}

/// Look up an account by DID.
pub async fn find_by_did(conn: &mut PgConnection, did: &str) -> Result<Option<Account>, sqlx::Error> {
    let row = sqlx::query("SELECT id, did, display_name, is_bot, recovery_blob FROM accounts WHERE did = $1")
        .bind(did)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(row.map(|r| Account {
        id: r.get("id"),
        did: r.get("did"),
        display_name: r.get("display_name"),
        is_bot: r.get("is_bot"),
        recovery_blob: r.get("recovery_blob"),
    }))
}

/// Get the recovery blob for a DID (unauthenticated access).
pub async fn get_recovery_blob(conn: &mut PgConnection, did: &str) -> Result<Option<Vec<u8>>, sqlx::Error> {
    let row = sqlx::query("SELECT recovery_blob FROM accounts WHERE did = $1")
        .bind(did)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(row.and_then(|r| r.get("recovery_blob")))
}

/// Update the recovery blob for an account.
pub async fn update_recovery_blob(
    conn: &mut PgConnection,
    account_id: i64,
    recovery_blob: Option<&[u8]>,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE accounts SET recovery_blob = $1 WHERE id = $2")
        .bind(recovery_blob)
        .bind(account_id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Hard-delete an account and all its data in a single transaction.
///
/// Deletes child rows in dependency order (children before parents) because
/// the schema has no ON DELETE CASCADE on its foreign keys.
///
/// Deletion order:
///   1. auth_challenges        (→ devices)
///   2. message_queue          (→ devices / sender_account_id)
///   3. push_pseudonyms        (→ devices)
///   4. session_tokens         (→ devices)
///   5. one_time_prekeys       (→ devices)
///   6. one_time_kyber_prekeys (→ devices)
///   7. signed_prekeys         (→ devices)
///   8. kyber_prekeys          (→ devices)
///   9. project_tokens         (→ accounts)
///  10. rate_limit_counters    (→ accounts)
///  11. did_documents          (→ accounts)
///  12. profiles               (→ accounts)
///  13. storage_items          (→ accounts)
///  14. storage_seq            (→ accounts)
///  15. storage_snapshots      (→ accounts)
///  16. project_bots           (→ accounts; Project row survives)
///  17. devices                (→ accounts)
///  18. accounts               (root)
pub async fn delete_account(conn: &mut PgConnection, account_id: i64) -> Result<(), sqlx::Error> {
    let mut tx = conn.begin().await?;

    // 1. auth_challenges references devices
    sqlx::query(
        "DELETE FROM auth_challenges \
         WHERE device_pk IN (SELECT id FROM devices WHERE account_id = $1)",
    )
    .bind(account_id)
    .execute(&mut *tx)
    .await?;

    // 2. message_queue: recipient_device_pk references devices; sender_account_id references accounts
    sqlx::query(
        "DELETE FROM message_queue \
         WHERE recipient_device_pk IN (SELECT id FROM devices WHERE account_id = $1) \
            OR sender_account_id = $1",
    )
    .bind(account_id)
    .execute(&mut *tx)
    .await?;

    // 3. push_pseudonyms references devices
    sqlx::query(
        "DELETE FROM push_pseudonyms \
         WHERE device_pk IN (SELECT id FROM devices WHERE account_id = $1)",
    )
    .bind(account_id)
    .execute(&mut *tx)
    .await?;

    // 4. session_tokens references devices
    sqlx::query(
        "DELETE FROM session_tokens \
         WHERE device_pk IN (SELECT id FROM devices WHERE account_id = $1)",
    )
    .bind(account_id)
    .execute(&mut *tx)
    .await?;

    // 5. one_time_prekeys references devices
    sqlx::query(
        "DELETE FROM one_time_prekeys \
         WHERE device_pk IN (SELECT id FROM devices WHERE account_id = $1)",
    )
    .bind(account_id)
    .execute(&mut *tx)
    .await?;

    // 6. one_time_kyber_prekeys references devices
    sqlx::query(
        "DELETE FROM one_time_kyber_prekeys \
         WHERE device_pk IN (SELECT id FROM devices WHERE account_id = $1)",
    )
    .bind(account_id)
    .execute(&mut *tx)
    .await?;

    // 7. signed_prekeys references devices
    sqlx::query(
        "DELETE FROM signed_prekeys \
         WHERE device_pk IN (SELECT id FROM devices WHERE account_id = $1)",
    )
    .bind(account_id)
    .execute(&mut *tx)
    .await?;

    // 8. kyber_prekeys references devices
    sqlx::query(
        "DELETE FROM kyber_prekeys \
         WHERE device_pk IN (SELECT id FROM devices WHERE account_id = $1)",
    )
    .bind(account_id)
    .execute(&mut *tx)
    .await?;

    // 9. project_tokens references accounts
    sqlx::query("DELETE FROM project_tokens WHERE account_id = $1")
        .bind(account_id)
        .execute(&mut *tx)
        .await?;

    // 10. rate_limit_counters references accounts
    sqlx::query("DELETE FROM rate_limit_counters WHERE account_id = $1")
        .bind(account_id)
        .execute(&mut *tx)
        .await?;

    // 11. did_documents references accounts
    sqlx::query("DELETE FROM did_documents WHERE account_id = $1")
        .bind(account_id)
        .execute(&mut *tx)
        .await?;

    // 12. profiles references accounts
    sqlx::query("DELETE FROM profiles WHERE account_id = $1")
        .bind(account_id)
        .execute(&mut *tx)
        .await?;

    // 13. storage_items references accounts (docs/05 storage service)
    sqlx::query("DELETE FROM storage_items WHERE account_id = $1")
        .bind(account_id)
        .execute(&mut *tx)
        .await?;

    // 14. storage_seq references accounts
    sqlx::query("DELETE FROM storage_seq WHERE account_id = $1")
        .bind(account_id)
        .execute(&mut *tx)
        .await?;

    // 15. storage_snapshots references accounts (docs/05 §7 passive backups)
    sqlx::query("DELETE FROM storage_snapshots WHERE account_id = $1")
        .bind(account_id)
        .execute(&mut *tx)
        .await?;

    // 16. project_bots references accounts (docs/24). Removing a bot account
    // unlinks it from its Project; the Project row itself survives (a bot can
    // be rotated/removed without deleting the Project).
    sqlx::query("DELETE FROM project_bots WHERE account_id = $1")
        .bind(account_id)
        .execute(&mut *tx)
        .await?;

    // 17. devices references accounts
    sqlx::query("DELETE FROM devices WHERE account_id = $1")
        .bind(account_id)
        .execute(&mut *tx)
        .await?;

    // 18. accounts (root row)
    sqlx::query("DELETE FROM accounts WHERE id = $1")
        .bind(account_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(())
}
