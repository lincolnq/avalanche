//! Project entity storage: the `projects`, `project_bots`, and the project's
//! signing key (docs/20, 24).
//!
//! A Project is a first-class server entity (distinct from a user/bot account).
//! It owns zero or more bot accounts (one-to-many via `project_bots`), holds a
//! signing key, and is the unit that capabilities attach to (see
//! [`crate::db::capabilities`]).
//!
//! The Project is addressed externally by its `slug` (stable; the token `iss`
//! stamp and the admin API path segment). The numeric `id` is internal.

use sqlx::{Acquire, PgConnection, Row};

pub struct Project {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub url: Option<String>,
    pub signing_public_key: Option<Vec<u8>>,
    /// OAuth "Sign in with Avalanche" client id (docs/25), if this Project offers
    /// login. `None` for the common case. Unique across Projects.
    pub oauth_client_id: Option<String>,
    /// Exact-match allowlist of `redirect_uri`s for this Project's login client.
    /// Empty unless `oauth_client_id` is set.
    pub oauth_redirect_uris: Vec<String>,
}

/// Create a Project. Returns its internal id. Errors on duplicate slug, or on a
/// duplicate `oauth_client_id` (the UNIQUE constraint — surfaced as a conflict
/// so a manifest cannot claim another Project's login client id).
pub async fn create(
    conn: &mut PgConnection,
    slug: &str,
    name: &str,
    url: Option<&str>,
    oauth_client_id: Option<&str>,
    oauth_redirect_uris: &[String],
) -> Result<i64, sqlx::Error> {
    let row = sqlx::query(
        "INSERT INTO projects (slug, name, url, oauth_client_id, oauth_redirect_uris)
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(slug)
    .bind(name)
    .bind(url)
    .bind(oauth_client_id)
    .bind(oauth_redirect_uris)
    .fetch_one(&mut *conn)
    .await?;
    Ok(row.get("id"))
}

/// Idempotently ensure the privileged adminbot Project row exists, returning
/// its id. Seeded at startup; its slug is the pin anchor for adminbot
/// authority. Headless (no url, no signing key).
pub async fn ensure_adminbot_project(
    conn: &mut PgConnection,
    slug: &str,
) -> Result<i64, sqlx::Error> {
    let row = sqlx::query(
        "INSERT INTO projects (slug, name) VALUES ($1, 'Adminbot')
         ON CONFLICT (slug) DO UPDATE SET slug = EXCLUDED.slug
         RETURNING id",
    )
    .bind(slug)
    .fetch_one(&mut *conn)
    .await?;
    Ok(row.get("id"))
}

/// Update an existing Project's installable fields (name, url, OAuth
/// registration) — the update half of an install upsert, so re-installing a
/// manifest for an existing slug refreshes it (docs/25) rather than being a
/// no-op. Leaves `slug` and `signing_public_key` untouched.
pub async fn update_registration(
    conn: &mut PgConnection,
    project_id: i64,
    name: &str,
    url: Option<&str>,
    oauth_client_id: Option<&str>,
    oauth_redirect_uris: &[String],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE projects
         SET name = $2, url = $3, oauth_client_id = $4, oauth_redirect_uris = $5
         WHERE id = $1",
    )
    .bind(project_id)
    .bind(name)
    .bind(url)
    .bind(oauth_client_id)
    .bind(oauth_redirect_uris)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn find_by_slug(
    conn: &mut PgConnection,
    slug: &str,
) -> Result<Option<Project>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, slug, name, url, signing_public_key, oauth_client_id, oauth_redirect_uris
         FROM projects WHERE slug = $1",
    )
    .bind(slug)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.map(row_to_project))
}

/// Resolve a Project by its OAuth login `client_id` (docs/25). `None` if no
/// Project registered that client id. Used by the OAuth endpoints to resolve a
/// login request's `client_id` to its token audience (`url`) + redirect allowlist.
pub async fn find_by_oauth_client_id(
    conn: &mut PgConnection,
    client_id: &str,
) -> Result<Option<Project>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, slug, name, url, signing_public_key, oauth_client_id, oauth_redirect_uris
         FROM projects WHERE oauth_client_id = $1",
    )
    .bind(client_id)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.map(row_to_project))
}

pub async fn list(conn: &mut PgConnection) -> Result<Vec<Project>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, slug, name, url, signing_public_key, oauth_client_id, oauth_redirect_uris
         FROM projects ORDER BY slug",
    )
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows.into_iter().map(row_to_project).collect())
}

fn row_to_project(r: sqlx::postgres::PgRow) -> Project {
    Project {
        id: r.get("id"),
        slug: r.get("slug"),
        name: r.get("name"),
        url: r.get("url"),
        signing_public_key: r.get("signing_public_key"),
        oauth_client_id: r.get("oauth_client_id"),
        oauth_redirect_uris: r.get("oauth_redirect_uris"),
    }
}

/// Delete a Project and its capability + bot-link rows in one transaction.
/// Returns whether a row existed. Does not delete the bot accounts themselves.
pub async fn delete_by_slug(conn: &mut PgConnection, slug: &str) -> Result<bool, sqlx::Error> {
    let mut tx = conn.begin().await?;
    let Some(row) = sqlx::query("SELECT id FROM projects WHERE slug = $1")
        .bind(slug)
        .fetch_optional(&mut *tx)
        .await?
    else {
        tx.rollback().await?;
        return Ok(false);
    };
    let project_id: i64 = row.get("id");

    sqlx::query("DELETE FROM project_capabilities WHERE project_id = $1")
        .bind(project_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM project_bots WHERE project_id = $1")
        .bind(project_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(project_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(true)
}

/// Set (or clear, with `None`) the Project's token-signing public key.
pub async fn set_signing_key(
    conn: &mut PgConnection,
    project_id: i64,
    key: Option<&[u8]>,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE projects SET signing_public_key = $1 WHERE id = $2")
        .bind(key)
        .bind(project_id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Link a bot account to a Project. A bot belongs to at most one Project
/// (account_id is the PK); re-linking moves it. Idempotent for the same pair.
pub async fn link_bot(
    conn: &mut PgConnection,
    project_id: i64,
    account_id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO project_bots (account_id, project_id) VALUES ($1, $2)
         ON CONFLICT (account_id) DO UPDATE SET project_id = EXCLUDED.project_id",
    )
    .bind(account_id)
    .bind(project_id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Link an account into a Project only if the Project has no linked account
/// yet — the superuser Project's one-time claim (docs/09 S-01). A single
/// conditional INSERT; it doesn't serialize two claims racing in the same
/// instant, which the gate's earlier check plus the claim needing the
/// never-shared superuser secret make moot. Returns whether the link was made.
pub async fn link_bot_if_unclaimed(
    conn: &mut PgConnection,
    project_id: i64,
    account_id: i64,
) -> Result<bool, sqlx::Error> {
    let res = sqlx::query(
        "INSERT INTO project_bots (account_id, project_id)
         SELECT $1, $2
         WHERE NOT EXISTS (SELECT 1 FROM project_bots WHERE project_id = $2)",
    )
    .bind(account_id)
    .bind(project_id)
    .execute(&mut *conn)
    .await?;
    Ok(res.rows_affected() > 0)
}

/// Remove a bot account's link to any Project. Returns whether a link existed.
pub async fn unlink_bot(conn: &mut PgConnection, account_id: i64) -> Result<bool, sqlx::Error> {
    let res = sqlx::query("DELETE FROM project_bots WHERE account_id = $1")
        .bind(account_id)
        .execute(&mut *conn)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// The DIDs of the bot accounts linked to a Project (for listing).
pub async fn bot_dids(conn: &mut PgConnection, project_id: i64) -> Result<Vec<String>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT a.did FROM project_bots pb JOIN accounts a ON a.id = pb.account_id
         WHERE pb.project_id = $1 ORDER BY a.did",
    )
    .bind(project_id)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows.into_iter().map(|r| r.get("did")).collect())
}

/// Resolve the Project a bot account belongs to, returning `(project_id,
/// slug)`. `None` if the account is not linked to any Project.
pub async fn project_for_account(
    conn: &mut PgConnection,
    account_id: i64,
) -> Result<Option<(i64, String)>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT p.id, p.slug FROM project_bots pb JOIN projects p ON p.id = pb.project_id
         WHERE pb.account_id = $1",
    )
    .bind(account_id)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.map(|r| (r.get("id"), r.get("slug"))))
}
