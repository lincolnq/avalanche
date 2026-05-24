//! Encrypted profile blob storage.
//!
//! The server stores opaque ciphertext. Only contacts who hold the user's
//! profile key (distributed via encrypted messages) can decrypt.

use sqlx::{PgConnection, Row};

/// Upsert an encrypted profile blob for an account.
pub async fn upsert(
    conn: &mut PgConnection,
    account_id: i64,
    encrypted_blob: &[u8],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO profiles (account_id, encrypted_blob, updated_at)
         VALUES ($1, $2, now())
         ON CONFLICT (account_id) DO UPDATE
            SET encrypted_blob = EXCLUDED.encrypted_blob,
                updated_at = now()",
    )
    .bind(account_id)
    .bind(encrypted_blob)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Fetch the encrypted blob for an account, if present.
pub async fn get_by_account_id(
    conn: &mut PgConnection,
    account_id: i64,
) -> Result<Option<Vec<u8>>, sqlx::Error> {
    let row = sqlx::query("SELECT encrypted_blob FROM profiles WHERE account_id = $1")
        .bind(account_id)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(row.map(|r| r.get("encrypted_blob")))
}
