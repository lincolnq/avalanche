//! Device-local avatar image cache (docs/55).
//!
//! Rebuildable from the server, so it lives in device.db (never synced; keeps
//! avatar bytes out of identity snapshots). One row per owner. `owner_kind`
//! separates account/DID-keyed avatars (own + contacts) from group-keyed ones;
//! `version` mirrors the owner's published avatar_version so app-core only
//! refetches when it advances.

use rusqlite::OptionalExtension as _;

use crate::{db::DeviceStore, error::StoreError};

/// Owner is an account (the user's own, or a contact), keyed by DID.
pub const AVATAR_KIND_ACCOUNT: i64 = 0;
/// Owner is a group, keyed by base64-url group id.
pub const AVATAR_KIND_GROUP: i64 = 1;

impl DeviceStore {
    /// Insert or replace the cached avatar bytes + version for an owner.
    pub async fn upsert_avatar(
        &self,
        owner_kind: i64,
        owner_id: &str,
        version: i64,
        bytes: &[u8],
    ) -> Result<(), StoreError> {
        let owner_id = owner_id.to_string();
        let bytes = bytes.to_vec();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO avatar_cache (owner_kind, owner_id, version, bytes)
                     VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![owner_kind, owner_id, version, bytes],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Load the cached avatar bytes for an owner, if any.
    pub async fn load_avatar(
        &self,
        owner_kind: i64,
        owner_id: &str,
    ) -> Result<Option<Vec<u8>>, StoreError> {
        let owner_id = owner_id.to_string();
        self.conn
            .call(move |conn| {
                conn.query_row(
                    "SELECT bytes FROM avatar_cache WHERE owner_kind = ?1 AND owner_id = ?2",
                    rusqlite::params![owner_kind, owner_id],
                    |row| row.get::<_, Vec<u8>>(0),
                )
                .optional()
                .map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Load just the cached avatar *version* for an owner (cheap refetch check).
    /// `None` means we've never cached one — caller should fetch if the owner
    /// advertises any avatar.
    pub async fn load_avatar_version(
        &self,
        owner_kind: i64,
        owner_id: &str,
    ) -> Result<Option<i64>, StoreError> {
        let owner_id = owner_id.to_string();
        self.conn
            .call(move |conn| {
                conn.query_row(
                    "SELECT version FROM avatar_cache WHERE owner_kind = ?1 AND owner_id = ?2",
                    rusqlite::params![owner_kind, owner_id],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Remove a cached avatar (idempotent).
    pub async fn delete_avatar(&self, owner_kind: i64, owner_id: &str) -> Result<(), StoreError> {
        let owner_id = owner_id.to_string();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "DELETE FROM avatar_cache WHERE owner_kind = ?1 AND owner_id = ?2",
                    rusqlite::params![owner_kind, owner_id],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory_split;

    #[tokio::test]
    async fn avatar_cache_round_trip() {
        let (_identity, device) = open_in_memory_split().await.unwrap();

        // Absent → None.
        assert!(device.load_avatar(AVATAR_KIND_ACCOUNT, "did:plc:a").await.unwrap().is_none());
        assert!(device.load_avatar_version(AVATAR_KIND_ACCOUNT, "did:plc:a").await.unwrap().is_none());

        // Insert, then read back bytes + version.
        device.upsert_avatar(AVATAR_KIND_ACCOUNT, "did:plc:a", 1, b"jpegbytes").await.unwrap();
        assert_eq!(
            device.load_avatar(AVATAR_KIND_ACCOUNT, "did:plc:a").await.unwrap().as_deref(),
            Some(&b"jpegbytes"[..])
        );
        assert_eq!(device.load_avatar_version(AVATAR_KIND_ACCOUNT, "did:plc:a").await.unwrap(), Some(1));

        // Overwrite bumps version + replaces bytes.
        device.upsert_avatar(AVATAR_KIND_ACCOUNT, "did:plc:a", 2, b"newbytes").await.unwrap();
        assert_eq!(device.load_avatar_version(AVATAR_KIND_ACCOUNT, "did:plc:a").await.unwrap(), Some(2));
        assert_eq!(
            device.load_avatar(AVATAR_KIND_ACCOUNT, "did:plc:a").await.unwrap().as_deref(),
            Some(&b"newbytes"[..])
        );

        // Group kind is a separate namespace from the same id string.
        device.upsert_avatar(AVATAR_KIND_GROUP, "did:plc:a", 9, b"groupavatar").await.unwrap();
        assert_eq!(device.load_avatar_version(AVATAR_KIND_ACCOUNT, "did:plc:a").await.unwrap(), Some(2));
        assert_eq!(device.load_avatar_version(AVATAR_KIND_GROUP, "did:plc:a").await.unwrap(), Some(9));

        // Delete.
        device.delete_avatar(AVATAR_KIND_ACCOUNT, "did:plc:a").await.unwrap();
        assert!(device.load_avatar(AVATAR_KIND_ACCOUNT, "did:plc:a").await.unwrap().is_none());
    }
}
