//! Locally-cached secrets that support recovery-blob updates without
//! re-authenticating the passkey.

use crate::db::IdentityStore;
use crate::error::StoreError;
use rusqlite::OptionalExtension;

impl IdentityStore {
    /// Persist the 32-byte PRF-derived recovery-blob symmetric key. Idempotent
    /// (single-row table; insert-or-replace). Called once at signup and
    /// again whenever the key is re-derived (e.g. recovery).
    pub async fn save_recovery_blob_key(&self, key: &[u8; 32]) -> Result<(), StoreError> {
        let key = key.to_vec();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO recovery_blob_key (id, key) VALUES (1, ?1)",
                    rusqlite::params![key],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Load the cached recovery-blob symmetric key. Returns `None` for
    /// accounts that opted out of passkey-based recovery (no key was ever
    /// stored).
    pub async fn load_recovery_blob_key(&self) -> Result<Option<[u8; 32]>, StoreError> {
        self.conn
            .call(|conn| {
                conn.query_row(
                    "SELECT key FROM recovery_blob_key WHERE id = 1",
                    [],
                    |row| row.get::<_, Vec<u8>>(0),
                )
                .optional()
                .map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)
            .and_then(|opt| match opt {
                None => Ok(None),
                Some(bytes) => bytes.as_slice().try_into().map(Some).map_err(|_| {
                    StoreError::Corrupt("stored recovery_blob_key length != 32".into())
                }),
            })
    }
}
