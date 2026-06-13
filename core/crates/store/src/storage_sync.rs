//! Storage-service client bookkeeping (docs/05-device-data-sync.md §3, §6).
//!
//! This is the local side of the durable-state sync engine: the identity-level
//! storage key, the per-record `storage_sync` sidecar (version / dirty /
//! tombstone — never any payload), and the single-row delta-pull cursor. The
//! engine itself (encrypt, push, pull, write-through) lives in `app-core`; this
//! module only persists and queries the bookkeeping.

use rusqlite::OptionalExtension as _;

use crate::{db::Store, error::StoreError};

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

impl Store {
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
    pub async fn set_storage_cursor(&self, seq: i64) -> Result<(), StoreError> {
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO storage_cursor (id, seq) VALUES (1, ?1)",
                    rusqlite::params![seq],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

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
}
