//! Local account identity and registration state.
//!
//! This module handles the two pieces of persistent state that exist before any
//! messages are sent:
//!
//! - **Identity key pair** — the long-term Ed25519 key pair generated at
//!   account creation. Stored in the `identity_keypair` table alongside the
//!   libsignal registration ID. This is also the data that
//!   [`libsignal_protocol::IdentityKeyStore::get_identity_key_pair`] reads when
//!   building outgoing messages (that implementation lives in [`crate::session`];
//!   the storage layer is shared via the same database connection).
//!
//! - **Registration info** — the account DID and homeserver URL confirmed after
//!   the server accepts the registration request. Absent until registration
//!   completes; `app-core` checks for its presence to decide whether to show the
//!   onboarding flow.

use rusqlite::OptionalExtension as _;
use types::Timestamp;

use crate::{db::Store, error::StoreError};

/// The local account state saved after successful registration.
#[derive(Debug, Clone)]
pub struct RegistrationInfo {
    pub account_id: String,
    pub server_url: String,
    pub registered_at: Timestamp,
    /// The local libsignal-style device_id assigned to this client. Currently
    /// always 1 (single-device), but threaded explicitly through registration
    /// + recovery so callers don't have to assume a fixed value.
    pub device_id: u32,
}

impl Store {
    /// Persist the local identity key pair and libsignal registration ID.
    /// Called once during account creation.
    pub async fn save_identity(
        &self,
        keypair: &crypto::IdentityKeyPair,
        registration_id: u32,
    ) -> Result<(), StoreError> {
        let bytes = keypair.serialize();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO identity_keypair (id, keypair_bytes, registration_id)
                     VALUES (1, ?1, ?2)",
                    rusqlite::params![bytes, registration_id],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Load the local identity key pair. Returns `None` if not yet created.
    pub async fn load_identity(
        &self,
    ) -> Result<Option<crypto::IdentityKeyPair>, StoreError> {
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

    /// Persist registration details after the homeserver confirms the account.
    pub async fn save_registration(&self, info: &RegistrationInfo) -> Result<(), StoreError> {
        let account_id = info.account_id.clone();
        let server_url = info.server_url.clone();
        let registered_at = info.registered_at.as_millis();
        let device_id = info.device_id;
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO account (id, account_id, server_url, registered_at, device_id)
                     VALUES (1, ?1, ?2, ?3, ?4)",
                    rusqlite::params![account_id, server_url, registered_at, device_id],
                )?;
                Ok(())
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

    /// Load registration details. Returns `None` if not yet registered.
    pub async fn load_registration(&self) -> Result<Option<RegistrationInfo>, StoreError> {
        let result = self
            .conn
            .call(|conn| {
                conn.query_row(
                    "SELECT account_id, server_url, registered_at, device_id
                     FROM account WHERE id = 1",
                    [],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, u32>(3)?,
                        ))
                    },
                )
                .optional()
                .map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)?;

        Ok(result.map(|(account_id, server_url, registered_at, device_id)| RegistrationInfo {
            account_id,
            server_url,
            registered_at: Timestamp(registered_at),
            device_id,
        }))
    }
}
