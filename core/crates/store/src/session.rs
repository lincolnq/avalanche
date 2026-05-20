//! libsignal store trait implementations.
//!
//! This module makes [`Store`] satisfy [`crypto::Store`] by implementing all
//! five libsignal store sub-traits:
//!
//! - [`libsignal_protocol::SessionStore`] — load and save Double Ratchet
//!   session records, keyed by remote address.
//! - [`libsignal_protocol::IdentityKeyStore`] — store the local identity key
//!   pair and track known remote identity keys for trust-on-first-use.
//! - [`libsignal_protocol::PreKeyStore`] — get, save, and remove one-time EC
//!   prekey records as they are consumed during session initiation.
//! - [`libsignal_protocol::SignedPreKeyStore`] — get and save the active signed
//!   EC prekey record.
//! - [`libsignal_protocol::KyberPreKeyStore`] — get, save, and mark-used Kyber
//!   post-quantum prekey records.
//!
//! All five implementations delegate to the SQLCipher database through
//! `self.conn.call(...)`, which runs the closure on the connection's dedicated
//! blocking thread. Errors from the store layer are converted to
//! `SignalProtocolError` via the `From` impl in [`crate::error`].
//!
//! The `crypto::Store` blanket impl at the bottom of this file ties everything
//! together: once all five sub-traits are satisfied, `Store` automatically
//! satisfies the combined bound.

use async_trait::async_trait;
use libsignal_protocol::{self as signal, GenericSignedPreKey as _};
use rusqlite::OptionalExtension as _;

use crate::{db::Store, error::StoreError};

// ── Helper ────────────────────────────────────────────────────────────────────

/// Encode a ProtocolAddress as "name.deviceId" for use as a table key.
fn addr_key(address: &signal::ProtocolAddress) -> String {
    format!("{}.{}", address.name(), u32::from(address.device_id()))
}

// ── SessionStore ──────────────────────────────────────────────────────────────

#[async_trait(?Send)]
impl signal::SessionStore for Store {
    async fn load_session(
        &self,
        address: &signal::ProtocolAddress,
    ) -> Result<Option<signal::SessionRecord>, signal::SignalProtocolError> {
        let key = addr_key(address);
        let result = self
            .conn
            .call(move |conn| {
                let mut stmt =
                    conn.prepare("SELECT record FROM sessions WHERE address = ?1")?;
                let mut rows = stmt.query([&key])?;
                Ok(rows.next()?.map(|row| {
                    let bytes: Vec<u8> = row.get(0)?;
                    Ok::<_, rusqlite::Error>(bytes)
                }))
            })
            .await
            .map_err(StoreError::Db)?;

        match result {
            Some(Ok(bytes)) => Ok(Some(
                signal::SessionRecord::deserialize(&bytes)?,
            )),
            Some(Err(e)) => Err(StoreError::Corrupt(e.to_string()).into()),
            None => Ok(None),
        }
    }

    async fn store_session(
        &mut self,
        address: &signal::ProtocolAddress,
        record: &signal::SessionRecord,
    ) -> Result<(), signal::SignalProtocolError> {
        let key = addr_key(address);
        let bytes = record.serialize()?;
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO sessions (address, record) VALUES (?1, ?2)",
                    rusqlite::params![key, bytes],
                )?;
                Ok(())
            })
            .await
            .map_err(|e| StoreError::Db(e).into())
    }
}

// ── IdentityKeyStore ──────────────────────────────────────────────────────────

#[async_trait(?Send)]
impl signal::IdentityKeyStore for Store {
    async fn get_identity_key_pair(
        &self,
    ) -> Result<signal::IdentityKeyPair, signal::SignalProtocolError> {
        let result = self
            .conn
            .call(|conn| {
                let mut stmt = conn
                    .prepare("SELECT keypair_bytes FROM identity_keypair WHERE id = 1")?;
                let mut rows = stmt.query([])?;
                Ok(rows.next()?.map(|row| {
                    let bytes: Vec<u8> = row.get(0)?;
                    Ok::<_, rusqlite::Error>(bytes)
                }))
            })
            .await
            .map_err(StoreError::Db)?;

        match result {
            Some(Ok(bytes)) => signal::IdentityKeyPair::try_from(bytes.as_slice()),
            Some(Err(e)) => Err(StoreError::Corrupt(e.to_string()).into()),
            None => Err(StoreError::NoIdentity.into()),
        }
    }

    async fn get_local_registration_id(
        &self,
    ) -> Result<u32, signal::SignalProtocolError> {
        let result = self
            .conn
            .call(|conn| {
                let mut stmt = conn
                    .prepare("SELECT registration_id FROM identity_keypair WHERE id = 1")?;
                let mut rows = stmt.query([])?;
                Ok(rows.next()?.map(|row| {
                    let id: u32 = row.get(0)?;
                    Ok::<_, rusqlite::Error>(id)
                }))
            })
            .await
            .map_err(StoreError::Db)?;

        match result {
            Some(Ok(id)) => Ok(id),
            Some(Err(e)) => Err(StoreError::Corrupt(e.to_string()).into()),
            None => Err(StoreError::NoIdentity.into()),
        }
    }

    async fn save_identity(
        &mut self,
        address: &signal::ProtocolAddress,
        identity: &signal::IdentityKey,
    ) -> Result<signal::IdentityChange, signal::SignalProtocolError> {
        let key = addr_key(address);
        let bytes = identity.serialize().to_vec();

        let changed = self
            .conn
            .call(move |conn| {
                let existing: Option<Vec<u8>> = conn
                    .query_row(
                        "SELECT identity_key FROM known_identities WHERE address = ?1",
                        [&key],
                        |row| row.get(0),
                    )
                    .optional()?;
                let changed = existing.as_deref() != Some(&bytes);
                conn.execute(
                    "INSERT OR REPLACE INTO known_identities (address, identity_key) VALUES (?1, ?2)",
                    rusqlite::params![key, bytes],
                )?;
                Ok(changed)
            })
            .await
            .map_err(StoreError::Db)?;

        Ok(signal::IdentityChange::from_changed(changed))
    }

    async fn is_trusted_identity(
        &self,
        address: &signal::ProtocolAddress,
        identity: &signal::IdentityKey,
        _direction: signal::Direction,
    ) -> Result<bool, signal::SignalProtocolError> {
        let key = addr_key(address);
        let incoming_bytes = identity.serialize().to_vec();

        let stored: Option<Vec<u8>> = self
            .conn
            .call(move |conn| {
                conn.query_row(
                    "SELECT identity_key FROM known_identities WHERE address = ?1",
                    [&key],
                    |row| row.get(0),
                )
                .optional()
                .map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)?;

        // Trust on first use: if no key stored yet, trust it.
        // If a key is stored, trust only if it matches.
        Ok(stored.is_none_or(|s| s == incoming_bytes))
    }

    async fn get_identity(
        &self,
        address: &signal::ProtocolAddress,
    ) -> Result<Option<signal::IdentityKey>, signal::SignalProtocolError> {
        let key = addr_key(address);

        let result: Option<Vec<u8>> = self
            .conn
            .call(move |conn| {
                conn.query_row(
                    "SELECT identity_key FROM known_identities WHERE address = ?1",
                    [&key],
                    |row| row.get(0),
                )
                .optional()
                .map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)?;

        match result {
            Some(bytes) => Ok(Some(
                signal::IdentityKey::decode(&bytes)?,
            )),
            None => Ok(None),
        }
    }
}

// ── PreKeyStore ───────────────────────────────────────────────────────────────

#[async_trait(?Send)]
impl signal::PreKeyStore for Store {
    async fn get_pre_key(
        &self,
        prekey_id: signal::PreKeyId,
    ) -> Result<signal::PreKeyRecord, signal::SignalProtocolError> {
        let id = u32::from(prekey_id);
        let result: Option<Vec<u8>> = self
            .conn
            .call(move |conn| {
                conn.query_row(
                    "SELECT record FROM prekeys WHERE id = ?1",
                    [id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)?;

        match result {
            Some(bytes) => signal::PreKeyRecord::deserialize(&bytes),
            None => Err(StoreError::PreKeyNotFound(id).into()),
        }
    }

    async fn save_pre_key(
        &mut self,
        prekey_id: signal::PreKeyId,
        record: &signal::PreKeyRecord,
    ) -> Result<(), signal::SignalProtocolError> {
        let id = u32::from(prekey_id);
        let bytes = record.serialize()?;
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO prekeys (id, record) VALUES (?1, ?2)",
                    rusqlite::params![id, bytes],
                )?;
                Ok(())
            })
            .await
            .map_err(|e| StoreError::Db(e).into())
    }

    async fn remove_pre_key(
        &mut self,
        prekey_id: signal::PreKeyId,
    ) -> Result<(), signal::SignalProtocolError> {
        let id = u32::from(prekey_id);
        self.conn
            .call(move |conn| {
                conn.execute("DELETE FROM prekeys WHERE id = ?1", [id])?;
                Ok(())
            })
            .await
            .map_err(|e| StoreError::Db(e).into())
    }
}

// ── SignedPreKeyStore ─────────────────────────────────────────────────────────

#[async_trait(?Send)]
impl signal::SignedPreKeyStore for Store {
    async fn get_signed_pre_key(
        &self,
        id: signal::SignedPreKeyId,
    ) -> Result<signal::SignedPreKeyRecord, signal::SignalProtocolError> {
        let id_u32 = u32::from(id);
        let result: Option<Vec<u8>> = self
            .conn
            .call(move |conn| {
                conn.query_row(
                    "SELECT record FROM signed_prekeys WHERE id = ?1",
                    [id_u32],
                    |row| row.get(0),
                )
                .optional()
                .map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)?;

        match result {
            Some(bytes) => signal::SignedPreKeyRecord::deserialize(&bytes),
            None => Err(StoreError::SignedPreKeyNotFound(id_u32).into()),
        }
    }

    async fn save_signed_pre_key(
        &mut self,
        id: signal::SignedPreKeyId,
        record: &signal::SignedPreKeyRecord,
    ) -> Result<(), signal::SignalProtocolError> {
        let id_u32 = u32::from(id);
        let bytes = record.serialize()?;
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO signed_prekeys (id, record) VALUES (?1, ?2)",
                    rusqlite::params![id_u32, bytes],
                )?;
                Ok(())
            })
            .await
            .map_err(|e| StoreError::Db(e).into())
    }
}

// ── KyberPreKeyStore ──────────────────────────────────────────────────────────

#[async_trait(?Send)]
impl signal::KyberPreKeyStore for Store {
    async fn get_kyber_pre_key(
        &self,
        id: signal::KyberPreKeyId,
    ) -> Result<signal::KyberPreKeyRecord, signal::SignalProtocolError> {
        let id_u32 = u32::from(id);
        let result: Option<Vec<u8>> = self
            .conn
            .call(move |conn| {
                conn.query_row(
                    "SELECT record FROM kyber_prekeys WHERE id = ?1",
                    [id_u32],
                    |row| row.get(0),
                )
                .optional()
                .map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)?;

        match result {
            Some(bytes) => signal::KyberPreKeyRecord::deserialize(&bytes),
            None => Err(StoreError::PreKeyNotFound(id_u32).into()),
        }
    }

    async fn save_kyber_pre_key(
        &mut self,
        id: signal::KyberPreKeyId,
        record: &signal::KyberPreKeyRecord,
    ) -> Result<(), signal::SignalProtocolError> {
        let id_u32 = u32::from(id);
        let bytes = record.serialize()?;
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO kyber_prekeys (id, record) VALUES (?1, ?2)",
                    rusqlite::params![id_u32, bytes],
                )?;
                Ok(())
            })
            .await
            .map_err(|e| StoreError::Db(e).into())
    }

    async fn mark_kyber_pre_key_used(
        &mut self,
        _id: signal::KyberPreKeyId,
        _ec_prekey_id: signal::SignedPreKeyId,
        _base_key: &signal::PublicKey,
    ) -> Result<(), signal::SignalProtocolError> {
        // Currently we only upload a single Kyber prekey per device, which
        // acts as a last-resort key — kept after use. Once we support a pool
        // of one-time Kyber prekeys with server-side atomic consumption,
        // one-time keys should be deleted here while the last-resort key
        // is kept.
        Ok(())
    }
}

// ── crypto::Store blanket impl ────────────────────────────────────────────────

impl crypto::Store for Store {}
