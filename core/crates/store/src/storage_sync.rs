//! Storage-service client bookkeeping (docs/05-device-data-sync.md §3, §6).
//!
//! This is the local side of the durable-state sync engine: the identity-level
//! storage key, the per-record `storage_sync` sidecar (version / dirty /
//! tombstone — never any payload), and the single-row delta-pull cursor. The
//! engine itself (encrypt, push, pull, write-through) lives in `app-core`; this
//! module only persists and queries the bookkeeping.

use rusqlite::OptionalExtension as _;

use crate::{
    db::{DeviceStore, IdentityStore},
    error::StoreError,
};

/// A dirty sidecar row awaiting push: identifies which domain record changed
/// and the server version we last saw (the CAS `expected_version`).
#[derive(Debug, Clone)]
pub struct DirtySyncRecord {
    pub type_tag: u16,
    pub logical_key: String,
    /// Server CAS token last seen (0 = never pushed → create-if-absent).
    pub version: i64,
    /// True when the domain row was deleted and a tombstone must be pushed.
    pub deleted: bool,
}

impl IdentityStore {
    /// Persist the 32-byte identity-level storage key. Called once at account
    /// creation and again on recovery/link when restored from the blob.
    pub async fn save_storage_key(&self, key: &[u8; 32]) -> Result<(), StoreError> {
        let key = key.to_vec();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO storage_key_state (id, storage_key) VALUES (1, ?1)",
                    rusqlite::params![key],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Load the storage key. Returns `None` if not yet provisioned.
    pub async fn load_storage_key(&self) -> Result<Option<[u8; 32]>, StoreError> {
        let bytes: Option<Vec<u8>> = self
            .conn
            .call(|conn| {
                conn.query_row(
                    "SELECT storage_key FROM storage_key_state WHERE id = 1",
                    [],
                    |row| row.get(0),
                )
                .optional()
                .map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)?;

        match bytes {
            None => Ok(None),
            Some(b) if b.len() == 32 => {
                let mut k = [0u8; 32];
                k.copy_from_slice(&b);
                Ok(Some(k))
            }
            Some(b) => Err(StoreError::Corrupt(format!(
                "storage_key is {} bytes, expected 32",
                b.len()
            ))),
        }
    }

}

impl DeviceStore {
    /// The highest server `seq` consumed so far (delta-pull cursor). 0 if unset.
    pub async fn storage_cursor(&self) -> Result<i64, StoreError> {
        let seq: Option<i64> = self
            .conn
            .call(|conn| {
                conn.query_row("SELECT seq FROM storage_cursor WHERE id = 1", [], |row| {
                    row.get(0)
                })
                .optional()
                .map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)?;
        Ok(seq.unwrap_or(0))
    }

    /// Advance the delta-pull cursor.
    ///
    /// No-ops when the cursor is already at `seq`. This is load-bearing for the
    /// commit-hook scheduler (docs/05 §6.1): the hook fires on every committed
    /// write, including the engine's own. If a no-op pull rewrote an unchanged
    /// cursor it would commit, re-poke the hook, and spin. Reading-then-skipping
    /// guarantees a settled `sync()` commits nothing and the loop quiesces.
    pub async fn set_storage_cursor(&self, seq: i64) -> Result<(), StoreError> {
        self.conn
            .call(move |conn| {
                let current: Option<i64> = conn
                    .query_row("SELECT seq FROM storage_cursor WHERE id = 1", [], |row| {
                        row.get(0)
                    })
                    .optional()?;
                if current == Some(seq) {
                    return Ok(());
                }
                conn.execute(
                    "INSERT OR REPLACE INTO storage_cursor (id, seq) VALUES (1, ?1)",
                    rusqlite::params![seq],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

}

impl IdentityStore {
    /// Every sidecar row with a local change pending push.
    pub async fn dirty_records(&self) -> Result<Vec<DirtySyncRecord>, StoreError> {
        self.conn
            .call(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT type, logical_key, version, deleted \
                     FROM storage_sync WHERE dirty = 1 ORDER BY type, logical_key",
                )?;
                let rows = stmt.query_map([], |row| {
                    Ok(DirtySyncRecord {
                        type_tag: row.get::<_, i64>(0)? as u16,
                        logical_key: row.get::<_, String>(1)?,
                        version: row.get::<_, i64>(2)?,
                        deleted: row.get::<_, i64>(3)? != 0,
                    })
                })?;
                rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Every sidecar row (regardless of dirty state) — the index of all synced
    /// records this device knows. `version`/`deleted` are meaningful per row;
    /// `dirty` is irrelevant.
    ///
    /// NOTE: currently only the *parked* `build_snapshot` path (docs/05 §7, under
    /// design review) needs whole-store enumeration, so this has no live caller
    /// right now. Kept because it's a generic read-only accessor the snapshot
    /// redesign will reuse; remove it if that path is dropped entirely.
    #[allow(dead_code)]
    pub async fn all_sync_records(&self) -> Result<Vec<DirtySyncRecord>, StoreError> {
        self.conn
            .call(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT type, logical_key, version, deleted \
                     FROM storage_sync ORDER BY type, logical_key",
                )?;
                let rows = stmt.query_map([], |row| {
                    Ok(DirtySyncRecord {
                        type_tag: row.get::<_, i64>(0)? as u16,
                        logical_key: row.get::<_, String>(1)?,
                        version: row.get::<_, i64>(2)?,
                        deleted: row.get::<_, i64>(3)? != 0,
                    })
                })?;
                rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)
    }

    /// The server version last recorded for a record (0 if unknown). Used for
    /// the pull-side last-writer-wins comparison.
    pub async fn sync_version(&self, type_tag: u16, logical_key: &str) -> Result<i64, StoreError> {
        let logical_key = logical_key.to_string();
        let v: Option<i64> = self
            .conn
            .call(move |conn| {
                conn.query_row(
                    "SELECT version FROM storage_sync WHERE type = ?1 AND logical_key = ?2",
                    rusqlite::params![type_tag as i64, logical_key],
                    |row| row.get(0),
                )
                .optional()
                .map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)?;
        Ok(v.unwrap_or(0))
    }

    /// Upsert the full sidecar state for a record. Used by the pull path after
    /// applying a record (`dirty = false`) and to record tombstones.
    pub async fn set_sync_meta(
        &self,
        type_tag: u16,
        logical_key: &str,
        version: i64,
        dirty: bool,
        deleted: bool,
    ) -> Result<(), StoreError> {
        let logical_key = logical_key.to_string();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO storage_sync (type, logical_key, version, dirty, deleted) \
                     VALUES (?1, ?2, ?3, ?4, ?5) \
                     ON CONFLICT(type, logical_key) DO UPDATE SET \
                       version = ?3, dirty = ?4, deleted = ?5",
                    rusqlite::params![
                        type_tag as i64,
                        logical_key,
                        version,
                        dirty as i64,
                        deleted as i64
                    ],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Mark a record clean after the server accepted a push: store the new
    /// version and clear `dirty`. The tombstone flag is left as-is.
    pub async fn set_sync_meta_clean(
        &self,
        type_tag: u16,
        logical_key: &str,
        version: i64,
    ) -> Result<(), StoreError> {
        let logical_key = logical_key.to_string();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "UPDATE storage_sync SET version = ?3, dirty = 0 \
                     WHERE type = ?1 AND logical_key = ?2",
                    rusqlite::params![type_tag as i64, logical_key, version],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Install the dirty-tracking triggers for the given synced types (§3.4).
    ///
    /// One `AFTER INSERT`/`AFTER UPDATE`/`AFTER DELETE` trigger per spec, each
    /// marking the matching `storage_sync` sidecar row dirty in the same
    /// transaction as the domain write — so feature code never has to remember
    /// to. Idempotent (`CREATE TRIGGER IF NOT EXISTS`), so it is safe to call on
    /// every store open.
    ///
    /// The specs come from the app-core sync registry — the single source of
    /// truth for `(table, key_column, type_tag)` — replacing what used to be
    /// hand-written SQL in `schema.rs`. Callers install these only when storage
    /// sync is enabled for the account (a storage key is present), so an
    /// opted-out account (e.g. a bot) accrues no sidecar rows.
    ///
    /// The fields are compile-time constants, never user input, so formatting
    /// them into DDL is not an injection surface.
    pub async fn install_sync_triggers(
        &self,
        specs: &[SyncTriggerSpec],
    ) -> Result<(), StoreError> {
        let sql = specs.iter().map(SyncTriggerSpec::ddl).collect::<String>();
        if sql.is_empty() {
            return Ok(());
        }
        self.conn
            .call(move |conn| {
                conn.execute_batch(&sql)?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Register a callback invoked on every committed write to the database
    /// (docs/05 §6.1 — the push scheduler's wake source).
    ///
    /// The callback runs on the connection's blocking thread and must be cheap
    /// and non-blocking — typically just poking a `Notify`. It replaces any
    /// previously-registered hook. Returning is fine; the commit always proceeds.
    pub async fn set_commit_hook<F>(&self, mut hook: F) -> Result<(), StoreError>
    where
        F: FnMut() + Send + 'static,
    {
        self.conn
            .call(move |conn| {
                conn.commit_hook(Some(move || {
                    hook();
                    false // false = allow the commit to proceed
                }));
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }
}

/// Declarative description of one synced type's dirty-tracking triggers (§3.4).
/// `table`/`key_column`/`type_tag` are all compile-time constants supplied by
/// the app-core adapters, so the generated DDL is fixed, not user-driven.
#[derive(Debug, Clone)]
pub struct SyncTriggerSpec {
    pub table: String,
    pub key_column: String,
    pub type_tag: u16,
}

impl SyncTriggerSpec {
    pub fn new(table: impl Into<String>, key_column: impl Into<String>, type_tag: u16) -> Self {
        Self {
            table: table.into(),
            key_column: key_column.into(),
            type_tag,
        }
    }

    /// The three `CREATE TRIGGER IF NOT EXISTS` statements for this type. INSERT
    /// and UPDATE mark the row dirty (clearing any stale tombstone); DELETE marks
    /// it dirty + tombstoned. Mirrors the hand-written groups triggers that used
    /// to live in `schema.rs`.
    fn ddl(&self) -> String {
        let Self {
            table,
            key_column,
            type_tag,
        } = self;
        format!(
            "CREATE TRIGGER IF NOT EXISTS {table}_sync_ai AFTER INSERT ON {table} BEGIN \
               INSERT INTO storage_sync(type, logical_key, dirty) VALUES ({type_tag}, NEW.{key_column}, 1) \
               ON CONFLICT(type, logical_key) DO UPDATE SET dirty = 1, deleted = 0; \
             END; \
             CREATE TRIGGER IF NOT EXISTS {table}_sync_au AFTER UPDATE ON {table} BEGIN \
               INSERT INTO storage_sync(type, logical_key, dirty) VALUES ({type_tag}, NEW.{key_column}, 1) \
               ON CONFLICT(type, logical_key) DO UPDATE SET dirty = 1, deleted = 0; \
             END; \
             CREATE TRIGGER IF NOT EXISTS {table}_sync_ad AFTER DELETE ON {table} BEGIN \
               INSERT INTO storage_sync(type, logical_key, dirty, deleted) VALUES ({type_tag}, OLD.{key_column}, 1, 1) \
               ON CONFLICT(type, logical_key) DO UPDATE SET dirty = 1, deleted = 1; \
             END; "
        )
    }
}
