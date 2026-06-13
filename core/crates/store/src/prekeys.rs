//! Prekey pool management.
//!
//! This module exposes the higher-level prekey operations that `app-core` uses
//! to manage the pools. The low-level single-record get/save/remove operations
//! required by libsignal's store traits live in [`crate::session`]; this module
//! sits above them and deals with batches and pool health.
//!
//! `app-core` is responsible for the refill policy: it calls
//! [`Store::remaining_one_time_prekey_count`] and
//! [`Store::remaining_kyber_prekey_count`] after each session initiation and
//! tops up the pools when either drops below a threshold (typically 10 keys).
//! The threshold is a policy decision, not enforced here.

use rusqlite::OptionalExtension;

use crate::{db::Store, error::StoreError};

/// First id handed out for a pool whose `prekey_counters` row doesn't exist
/// yet. Stores created before this allocator existed only ever issued ids
/// `1..=20` at registration (no replenishment ran), so 21 is the first safe
/// id. Registration explicitly seeds the cursor for fresh stores; this default
/// only covers pre-existing ones.
const DEFAULT_NEXT_PREKEY_ID: i64 = 21;

impl Store {
    /// Save a batch of generated one-time prekey records to the pool.
    pub async fn save_one_time_prekeys(
        &self,
        records: &[(u32, Vec<u8>)],
    ) -> Result<(), StoreError> {
        let records = records.to_vec();
        self.conn
            .call(move |conn| {
                let tx = conn.transaction()?;
                for (id, record) in &records {
                    tx.execute(
                        "INSERT OR REPLACE INTO prekeys (id, record) VALUES (?1, ?2)",
                        rusqlite::params![id, record],
                    )?;
                }
                tx.commit()?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Number of one-time prekeys remaining in the pool.
    /// The app should refill when this drops below a threshold (typically 10).
    pub async fn remaining_one_time_prekey_count(&self) -> Result<usize, StoreError> {
        let count: i64 = self
            .conn
            .call(|conn| {
                conn.query_row("SELECT COUNT(*) FROM prekeys", [], |row| row.get(0))
                    .map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)?;
        Ok(count as usize)
    }

    /// Save the active signed prekey record.
    pub async fn save_signed_prekey(
        &self,
        id: u32,
        record: &[u8],
    ) -> Result<(), StoreError> {
        let record = record.to_vec();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO signed_prekeys (id, record) VALUES (?1, ?2)",
                    rusqlite::params![id, record],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Save a batch of generated Kyber prekey records to the pool.
    pub async fn save_kyber_prekeys(
        &self,
        records: &[(u32, Vec<u8>)],
    ) -> Result<(), StoreError> {
        let records = records.to_vec();
        self.conn
            .call(move |conn| {
                let tx = conn.transaction()?;
                for (id, record) in &records {
                    tx.execute(
                        "INSERT OR REPLACE INTO kyber_prekeys (id, record) VALUES (?1, ?2)",
                        rusqlite::params![id, record],
                    )?;
                }
                tx.commit()?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Number of Kyber prekeys remaining in the pool.
    pub async fn remaining_kyber_prekey_count(&self) -> Result<usize, StoreError> {
        let count: i64 = self
            .conn
            .call(|conn| {
                conn.query_row("SELECT COUNT(*) FROM kyber_prekeys", [], |row| row.get(0))
                    .map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)?;
        Ok(count as usize)
    }

    /// Reserve a contiguous block of `count` prekey ids for `kind`
    /// (`"one_time"` for EC, `"kyber"`) and advance the persistent cursor past
    /// them, returning the first id of the block. The read-and-bump is one
    /// transaction, so concurrent callers never get overlapping ranges.
    ///
    /// Ids are never reused even after the keys are consumed and deleted from
    /// the pool — reuse could let a stale PreKey/Kyber message match a
    /// freshly-generated key.
    pub async fn allocate_prekey_ids(&self, kind: &str, count: u32) -> Result<u32, StoreError> {
        let kind = kind.to_string();
        self.conn
            .call(move |conn| {
                let tx = conn.transaction()?;
                let start: i64 = tx
                    .query_row(
                        "SELECT next_id FROM prekey_counters WHERE kind = ?1",
                        rusqlite::params![kind],
                        |row| row.get(0),
                    )
                    .optional()?
                    .unwrap_or(DEFAULT_NEXT_PREKEY_ID);
                tx.execute(
                    "INSERT OR REPLACE INTO prekey_counters (kind, next_id) VALUES (?1, ?2)",
                    rusqlite::params![kind, start + count as i64],
                )?;
                tx.commit()?;
                Ok(start as u32)
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Seed the next-id cursor for `kind` if it has no row yet (called once at
    /// registration). Idempotent — leaves an already-advanced cursor untouched,
    /// so re-running schema/registration logic can't rewind the allocator.
    pub async fn seed_prekey_counter(&self, kind: &str, next_id: u32) -> Result<(), StoreError> {
        let kind = kind.to_string();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT OR IGNORE INTO prekey_counters (kind, next_id) VALUES (?1, ?2)",
                    rusqlite::params![kind, next_id as i64],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }
}

#[cfg(test)]
mod tests {
    use crate::Store;

    async fn mem_store() -> Store {
        Store::open_in_memory().await.expect("open in-memory store")
    }

    #[tokio::test]
    async fn allocate_defaults_to_21_then_advances_monotonically() {
        let store = mem_store().await;
        // Un-seeded pool starts at the pre-existing-store default (21).
        assert_eq!(store.allocate_prekey_ids("one_time", 5).await.unwrap(), 21);
        // Next block is contiguous and non-overlapping.
        assert_eq!(store.allocate_prekey_ids("one_time", 3).await.unwrap(), 26);
        assert_eq!(store.allocate_prekey_ids("one_time", 1).await.unwrap(), 29);
    }

    #[tokio::test]
    async fn counters_are_independent_per_kind() {
        let store = mem_store().await;
        assert_eq!(store.allocate_prekey_ids("one_time", 4).await.unwrap(), 21);
        // A different kind has its own cursor.
        assert_eq!(store.allocate_prekey_ids("kyber", 4).await.unwrap(), 21);
        assert_eq!(store.allocate_prekey_ids("one_time", 1).await.unwrap(), 25);
        assert_eq!(store.allocate_prekey_ids("kyber", 1).await.unwrap(), 25);
    }

    #[tokio::test]
    async fn seed_sets_start_and_is_idempotent() {
        let store = mem_store().await;
        store.seed_prekey_counter("kyber", 22).await.unwrap();
        // A second seed must not rewind an already-set cursor.
        store.seed_prekey_counter("kyber", 5).await.unwrap();
        assert_eq!(store.allocate_prekey_ids("kyber", 2).await.unwrap(), 22);
        assert_eq!(store.allocate_prekey_ids("kyber", 1).await.unwrap(), 24);
    }
}
