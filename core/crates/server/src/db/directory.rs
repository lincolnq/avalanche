//! Directory entries: the client-facing "Network tab" project directory
//! (docs/20, docs/22).
//!
//! `GET /v1/projects` reads [`list`]; adminbot's `/install-project` manifest
//! drives per-Project entries via [`replace_for_project`]; a one-time startup
//! seed ([`is_empty`] + [`seed`]) migrates any legacy `PROJECTS` env content in.
//!
//! An entry with `project_id = Some(..)` belongs to an installed Project and is
//! dropped by `ON DELETE CASCADE` when that Project is uninstalled; an entry
//! with `project_id = None` is an operator/seeded row managed directly.
//!
//! This table has NO did/account_id column — it is directory metadata only, so
//! the membership-opacity discipline (docs/03 §3.9) does not apply.

use sqlx::{Acquire, PgConnection, Row};

/// A directory row as served to clients (the `GET /v1/projects` shape).
pub struct DirectoryEntry {
    pub name: String,
    pub url: String,
    pub description: String,
    pub client_id: Option<String>,
    pub official: bool,
}

/// A manifest-driven entry to (re)write for a Project. Officialness and OAuth
/// client id are not accepted from this path (always non-official, no client
/// id) — see the module docs.
pub struct ProjectEntry {
    pub name: String,
    pub url: String,
    pub description: String,
}

/// A legacy `PROJECTS`-env entry to seed once at startup, preserving its
/// operator-set `official` flag and OAuth `client_id`.
pub struct SeedEntry {
    pub name: String,
    pub url: String,
    pub description: String,
    pub client_id: Option<String>,
    pub official: bool,
}

/// Every directory entry, in display order.
pub async fn list(conn: &mut PgConnection) -> Result<Vec<DirectoryEntry>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT name, url, description, client_id, official
         FROM directory_entries ORDER BY position, id",
    )
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| DirectoryEntry {
            name: r.get("name"),
            url: r.get("url"),
            description: r.get("description"),
            client_id: r.get("client_id"),
            official: r.get("official"),
        })
        .collect())
}

/// Replace the full set of directory entries for one Project (delete-then-insert
/// in a transaction). Entries are stored non-official with no client id and in
/// the given order.
pub async fn replace_for_project(
    conn: &mut PgConnection,
    project_id: i64,
    entries: &[ProjectEntry],
) -> Result<(), sqlx::Error> {
    let mut tx = conn.begin().await?;
    sqlx::query("DELETE FROM directory_entries WHERE project_id = $1")
        .bind(project_id)
        .execute(&mut *tx)
        .await?;
    for (i, e) in entries.iter().enumerate() {
        sqlx::query(
            "INSERT INTO directory_entries (project_id, name, url, description, position)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(project_id)
        .bind(&e.name)
        .bind(&e.url)
        .bind(&e.description)
        .bind(i as i32)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Whether the directory table has no rows yet (the one-time startup seed
/// guard — once seeded, later `PROJECTS` env edits are ignored).
pub async fn is_empty(conn: &mut PgConnection) -> Result<bool, sqlx::Error> {
    let row = sqlx::query("SELECT EXISTS (SELECT 1 FROM directory_entries) AS present")
        .fetch_one(&mut *conn)
        .await?;
    let present: bool = row.get("present");
    Ok(!present)
}

/// Seed operator entries (from the legacy `PROJECTS` env var) as unowned
/// (`project_id = NULL`) rows, preserving their `official`/`client_id`. Caller
/// guards on [`is_empty`].
pub async fn seed(conn: &mut PgConnection, entries: &[SeedEntry]) -> Result<(), sqlx::Error> {
    let mut tx = conn.begin().await?;
    for (i, e) in entries.iter().enumerate() {
        sqlx::query(
            "INSERT INTO directory_entries
                (project_id, name, url, description, client_id, official, position)
             VALUES (NULL, $1, $2, $3, $4, $5, $6)",
        )
        .bind(&e.name)
        .bind(&e.url)
        .bind(&e.description)
        .bind(&e.client_id)
        .bind(e.official)
        .bind(i as i32)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}
