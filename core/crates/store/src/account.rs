//! Local account identity and registration state, split across the two stores.
//!
//! - **Identity (identity.db):** the long-term identity key pair, the DID, and
//!   the P-256 rotation key — the same on every device of the identity.
//! - **Device (device.db):** this device's registration — the libsignal
//!   registration_id plus the `(server_url, device_id)` this account context is
//!   bound to.
//!
//! The identity key pair is read by [`libsignal_protocol::IdentityKeyStore`]
//! (impl in [`crate::session`], which reaches identity.db via
//! [`crate::DeviceStore::identity`]); the registration_id is read by
//! `get_local_registration_id` from device.db.

use rusqlite::OptionalExtension as _;
use types::Timestamp;

use crate::{
    db::{DeviceStore, IdentityStore},
    error::StoreError,
};

/// This device's registration row (device.db).
#[derive(Debug, Clone)]
pub struct DeviceAccount {
    pub server_url: String,
    pub device_id: u32,
    pub registered_at: Timestamp,
    pub registration_id: u32,
}

impl IdentityStore {
    /// Persist the local identity key pair. Called once at account creation /
    /// recovery.
    pub async fn save_identity_keypair(
        &self,
        keypair: &crypto::IdentityKeyPair,
    ) -> Result<(), StoreError> {
        let bytes = keypair.serialize();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO identity_keypair (id, keypair_bytes) VALUES (1, ?1)",
                    rusqlite::params![bytes],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Load the local identity key pair. Returns `None` if not yet created.
    pub async fn load_identity(&self) -> Result<Option<crypto::IdentityKeyPair>, StoreError> {
        let result: Option<Vec<u8>> = self
            .conn
            .call(|conn| {
                conn.query_row(
                    "SELECT keypair_bytes FROM identity_keypair WHERE id = 1",
                    [],
                    |row| row.get(0),
                )
                .optional()
                .map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)?;

        match result {
            Some(bytes) => crypto::IdentityKeyPair::deserialize(&bytes)
                .map(Some)
                .map_err(|e| StoreError::Corrupt(e.to_string())),
            None => Ok(None),
        }
    }

    /// Persist the identity's DID (and when it was first established locally).
    pub async fn save_did(&self, did: &str, registered_at: Timestamp) -> Result<(), StoreError> {
        let did = did.to_string();
        let registered_at = registered_at.as_millis();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO account_identity (id, did, registered_at)
                     VALUES (1, ?1, ?2)",
                    rusqlite::params![did, registered_at],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Load the identity's DID and establishment time. `None` until registered.
    pub async fn load_did(&self) -> Result<Option<(String, Timestamp)>, StoreError> {
        self.conn
            .call(|conn| {
                conn.query_row(
                    "SELECT did, registered_at FROM account_identity WHERE id = 1",
                    [],
                    |row| Ok((row.get::<_, String>(0)?, Timestamp(row.get::<_, i64>(1)?))),
                )
                .optional()
                .map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Save the P-256 rotation key (private + public halves).
    pub async fn save_rotation_key(
        &self,
        private_key: &[u8],
        public_key: &[u8],
    ) -> Result<(), StoreError> {
        let priv_bytes = private_key.to_vec();
        let pub_bytes = public_key.to_vec();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO rotation_key (id, private_key, public_key)
                     VALUES (1, ?1, ?2)",
                    rusqlite::params![priv_bytes, pub_bytes],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Load the P-256 rotation key. Returns `None` if not yet generated.
    pub async fn load_rotation_key(&self) -> Result<Option<(Vec<u8>, Vec<u8>)>, StoreError> {
        self.conn
            .call(|conn| {
                conn.query_row(
                    "SELECT private_key, public_key FROM rotation_key WHERE id = 1",
                    [],
                    |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?)),
                )
                .optional()
                .map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Irreversibly clear **all** durable per-identity state from identity.db:
    /// the identity keypair, DID, rotation key, storage key, recovery-blob key,
    /// contacts, groups (master keys), profiles, message history, and the
    /// storage-sync sidecar. Used by the Delete-identity flow (docs/53) after the
    /// server accounts are deleted and the DID is tombstoned. The file itself
    /// remains (the embedder is responsible for deleting it); after this call no
    /// key material or readable content survives in it.
    ///
    /// Table list must stay in sync with [`crate::schema::IDENTITY_TABLES`] /
    /// [`crate::schema::IDENTITY_MIGRATIONS`].
    pub async fn wipe_identity(&self) -> Result<(), StoreError> {
        self.conn
            .call(|conn| {
                // No inter-table FKs in identity.db, so deletion order is
                // irrelevant; one transaction keeps it all-or-nothing.
                conn.execute_batch(
                    "BEGIN;
                     DELETE FROM identity_keypair;
                     DELETE FROM account_identity;
                     DELETE FROM known_identities;
                     DELETE FROM rotation_key;
                     DELETE FROM own_profile;
                     DELETE FROM contact_profiles;
                     DELETE FROM groups;
                     DELETE FROM contacts;
                     DELETE FROM conversation_settings;
                     DELETE FROM recovery_blob_key;
                     DELETE FROM message_history;
                     DELETE FROM message_revisions;
                     DELETE FROM reactions;
                     DELETE FROM account_info_cache;
                     DELETE FROM storage_key_state;
                     DELETE FROM storage_sync;
                     COMMIT;",
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }
}

impl DeviceStore {
    /// Persist this device's registration (server binding + registration_id).
    pub async fn save_device_account(&self, info: &DeviceAccount) -> Result<(), StoreError> {
        let server_url = info.server_url.clone();
        let device_id = info.device_id;
        let registered_at = info.registered_at.as_millis();
        let registration_id = info.registration_id;
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO device_account
                       (id, server_url, device_id, registered_at, registration_id)
                     VALUES (1, ?1, ?2, ?3, ?4)",
                    rusqlite::params![server_url, device_id, registered_at, registration_id],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Load this device's registration. Returns `None` if not yet registered.
    pub async fn load_device_account(&self) -> Result<Option<DeviceAccount>, StoreError> {
        self.conn
            .call(|conn| {
                conn.query_row(
                    "SELECT server_url, device_id, registered_at, registration_id
                     FROM device_account WHERE id = 1",
                    [],
                    |row| {
                        Ok(DeviceAccount {
                            server_url: row.get::<_, String>(0)?,
                            device_id: row.get::<_, u32>(1)?,
                            registered_at: Timestamp(row.get::<_, i64>(2)?),
                            registration_id: row.get::<_, u32>(3)?,
                        })
                    },
                )
                .optional()
                .map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Irreversibly clear **all** per-device transport crypto and server-bound
    /// caches from device.db: Double Ratchet sessions, prekey pools, sender keys,
    /// push state, credential/param caches, the outbound queue, the storage
    /// cursor, and this device's registration. Used by the Delete-identity flow
    /// (docs/53) per account context. The file itself remains (the embedder
    /// deletes it); after this call no ratchet or key material survives in it.
    ///
    /// Table list must stay in sync with [`crate::schema::DEVICE_TABLES`] /
    /// [`crate::schema::DEVICE_MIGRATIONS`] (plus `device_account`, which
    /// post-dates the legacy-migration constant).
    pub async fn wipe_device(&self) -> Result<(), StoreError> {
        self.conn
            .call(|conn| {
                conn.execute_batch(
                    "BEGIN;
                     DELETE FROM sessions;
                     DELETE FROM prekeys;
                     DELETE FROM signed_prekeys;
                     DELETE FROM kyber_prekeys;
                     DELETE FROM prekey_counters;
                     DELETE FROM sender_keys;
                     DELETE FROM sender_key_shared;
                     DELETE FROM push_state;
                     DELETE FROM group_credentials;
                     DELETE FROM group_server_params;
                     DELETE FROM profile_fetch_state;
                     DELETE FROM message_queue;
                     DELETE FROM storage_cursor;
                     DELETE FROM device_account;
                     COMMIT;",
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }
}
