//! Account storage: create and look up accounts by DID.
//!
//! An account represents a user identity on this homeserver. The external
//! identifier is a `did:plc:...` string; the internal `id` (bigint) is used
//! as a foreign key throughout the schema but never exposed in the API.

use sqlx::{PgConnection, Row};

pub struct Account {
    pub id: i64,
    pub did: String,
    pub display_name: Option<String>,
    pub is_bot: bool,
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
    let row = sqlx::query("SELECT id, did, display_name, is_bot FROM accounts WHERE did = $1")
        .bind(did)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(row.map(|r| Account {
        id: r.get("id"),
        did: r.get("did"),
        display_name: r.get("display_name"),
        is_bot: r.get("is_bot"),
    }))
}
