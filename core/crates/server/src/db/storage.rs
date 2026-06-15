//! Storage service DB layer (docs/05-device-data-sync.md §5).
//!
//! The server is type-blind: it stores opaque `(record_id, ciphertext)` pairs
//! per account, orders writes with a per-account monotonic counter, and enforces
//! per-record CAS via the `version` column. All semantics live in the client.
//!
//! Both `version` (per-record CAS token) and `seq` (per-account cursor space)
//! are sourced from the single `storage_seq` counter, so every applied write
//! gets a fresh account-unique, monotonically increasing value.

use sqlx::{PgConnection, Row};

/// A stored record as returned by a delta pull.
pub struct StorageItem {
    pub record_id: Vec<u8>,
    pub version: i64,
    pub seq: i64,
    pub deleted: bool,
    pub ciphertext: Vec<u8>,
}

/// Outcome of a single CAS write.
pub enum PutOutcome {
    /// The write applied; carries the freshly allocated version/seq.
    Applied { version: i64, seq: i64 },
    /// The write was rejected: `expected_version` did not match the stored
    /// version (0 = the record does not exist). Returned un-applied; the client
    /// re-pulls and retries (§5).
    Conflict { current_version: i64 },
}

/// Current live usage for an account: (total ciphertext bytes, live record count).
/// Tombstones (`deleted = TRUE`) are excluded — they carry no payload and are not
/// live records.
pub async fn account_usage(
    conn: &mut PgConnection,
    account_id: i64,
) -> Result<(i64, i64), sqlx::Error> {
    let row = sqlx::query(
        "SELECT COALESCE(SUM(byte_len), 0)::BIGINT AS total, COUNT(*)::BIGINT AS cnt \
         FROM storage_items WHERE account_id = $1 AND deleted = FALSE",
    )
    .bind(account_id)
    .fetch_one(&mut *conn)
    .await?;
    Ok((row.get("total"), row.get("cnt")))
}

/// Delta pull: every record with `seq > since`, ordered by `seq`, up to `limit`.
pub async fn pull(
    conn: &mut PgConnection,
    account_id: i64,
    since: i64,
    limit: i64,
) -> Result<Vec<StorageItem>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT record_id, version, seq, deleted, ciphertext \
         FROM storage_items \
         WHERE account_id = $1 AND seq > $2 \
         ORDER BY seq ASC \
         LIMIT $3",
    )
    .bind(account_id)
    .bind(since)
    .bind(limit)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| StorageItem {
            record_id: r.get("record_id"),
            version: r.get("version"),
            seq: r.get("seq"),
            deleted: r.get("deleted"),
            ciphertext: r.get("ciphertext"),
        })
        .collect())
}

/// Allocate the next per-account counter value. Atomic via upsert; the first
/// allocation for an account returns 1, so 0 stays reserved for "create-if-absent"
/// (`expected_version = 0`) and full pulls (`since = 0`).
pub async fn alloc_seq(conn: &mut PgConnection, account_id: i64) -> Result<i64, sqlx::Error> {
    let row = sqlx::query(
        "INSERT INTO storage_seq (account_id, next_seq) VALUES ($1, 1) \
         ON CONFLICT (account_id) DO UPDATE SET next_seq = storage_seq.next_seq + 1 \
         RETURNING next_seq",
    )
    .bind(account_id)
    .fetch_one(&mut *conn)
    .await?;
    Ok(row.get("next_seq"))
}

/// CAS write of a single record. Must be called inside a transaction so the
/// version check, counter bump, and upsert are atomic.
///
/// `expected_version = 0` means create-if-absent. A tombstone passes
/// `deleted = true` and an empty `ciphertext`.
pub async fn put_item(
    conn: &mut PgConnection,
    account_id: i64,
    record_id: &[u8],
    expected_version: i64,
    deleted: bool,
    ciphertext: &[u8],
) -> Result<PutOutcome, sqlx::Error> {
    // Lock the row if it exists so a concurrent writer can't slip between the
    // CAS check and the upsert.
    let current: Option<i64> = sqlx::query(
        "SELECT version FROM storage_items \
         WHERE account_id = $1 AND record_id = $2 FOR UPDATE",
    )
    .bind(account_id)
    .bind(record_id)
    .fetch_optional(&mut *conn)
    .await?
    .map(|r| r.get("version"));

    // CAS: absent row is version 0; mismatch is a conflict, returned un-applied.
    let stored = current.unwrap_or(0);
    if stored != expected_version {
        return Ok(PutOutcome::Conflict {
            current_version: stored,
        });
    }

    let seq = alloc_seq(conn, account_id).await?;
    sqlx::query(
        "INSERT INTO storage_items \
           (account_id, record_id, version, seq, ciphertext, deleted, byte_len, updated_at) \
         VALUES ($1, $2, $3, $3, $4, $5, $6, now()) \
         ON CONFLICT (account_id, record_id) DO UPDATE SET \
           version = $3, seq = $3, ciphertext = $4, deleted = $5, byte_len = $6, updated_at = now()",
    )
    .bind(account_id)
    .bind(record_id)
    .bind(seq)
    .bind(ciphertext)
    .bind(deleted)
    .bind(ciphertext.len() as i32)
    .execute(&mut *conn)
    .await?;

    Ok(PutOutcome::Applied { version: seq, seq })
}

// ── Snapshots (docs/05 §5/§7) ────────────────────────────────────────────────
//
// A whole-store blob per account for the passive backups: single-authoritative
// live items on the discovery server, one-way snapshots on the rest. Opaque to
// the server (encrypted under the identity storage key) and conflict-free —
// last-writer-wins on `snapshot_version`.

/// A stored whole-store snapshot.
pub struct Snapshot {
    pub snapshot_version: i64,
    pub blob: Vec<u8>,
}

/// Outcome of a snapshot PUT.
pub enum SnapshotOutcome {
    /// The incoming snapshot won LWW and is now stored.
    Stored { snapshot_version: i64 },
    /// The incoming `snapshot_version` was not strictly newer than the held one;
    /// nothing was written. Carries the version currently stored.
    Stale { current_version: i64 },
}

/// Fetch the account's snapshot, if any.
pub async fn get_snapshot(
    conn: &mut PgConnection,
    account_id: i64,
) -> Result<Option<Snapshot>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT snapshot_version, blob FROM storage_snapshots WHERE account_id = $1",
    )
    .bind(account_id)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.map(|r| Snapshot {
        snapshot_version: r.get("snapshot_version"),
        blob: r.get("blob"),
    }))
}

/// Store a snapshot under last-writer-wins on `snapshot_version`: the blob is
/// written iff its version is strictly newer than the one held (or none exists).
/// A stale push leaves the stored snapshot untouched. Atomic in a single
/// statement — the `ON CONFLICT … WHERE` only updates when the incoming version
/// wins, and `RETURNING` is empty when the update is skipped.
pub async fn put_snapshot(
    conn: &mut PgConnection,
    account_id: i64,
    snapshot_version: i64,
    blob: &[u8],
) -> Result<SnapshotOutcome, sqlx::Error> {
    let stored: Option<i64> = sqlx::query(
        "INSERT INTO storage_snapshots (account_id, snapshot_version, blob, updated_at) \
         VALUES ($1, $2, $3, now()) \
         ON CONFLICT (account_id) DO UPDATE \
           SET snapshot_version = EXCLUDED.snapshot_version, \
               blob = EXCLUDED.blob, \
               updated_at = now() \
           WHERE EXCLUDED.snapshot_version > storage_snapshots.snapshot_version \
         RETURNING snapshot_version",
    )
    .bind(account_id)
    .bind(snapshot_version)
    .bind(blob)
    .fetch_optional(&mut *conn)
    .await?
    .map(|r| r.get("snapshot_version"));

    match stored {
        Some(v) => Ok(SnapshotOutcome::Stored { snapshot_version: v }),
        None => {
            // The update was skipped because the incoming version did not win.
            // Read back the version currently held to report it.
            let current: i64 = sqlx::query(
                "SELECT snapshot_version FROM storage_snapshots WHERE account_id = $1",
            )
            .bind(account_id)
            .fetch_one(&mut *conn)
            .await?
            .get("snapshot_version");
            Ok(SnapshotOutcome::Stale {
                current_version: current,
            })
        }
    }
}
