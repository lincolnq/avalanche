//! Profile key and cached contact profile storage.
//!
//! Two distinct things live here:
//!
//! - **Own profile** (`own_profile` table) — the 32-byte profile key generated
//!   at account creation, plus a plaintext copy of the user's own display name.
//!   The profile key controls who can decrypt the user's encrypted blob on the
//!   server; the local display name is cached so the UI doesn't need to fetch
//!   and decrypt its own blob.
//!
//! - **Contact profiles** (`contact_profiles` table) — decrypted display names
//!   keyed by DID, alongside the profile key the contact shared with us. Acts
//!   as a fast lookup for the conversation list and message bubbles, and as a
//!   "is this profile key new?" check on inbound messages.

use rusqlite::OptionalExtension as _;
use types::Timestamp;

use crate::{db::Store, error::StoreError};

/// Own profile state: 32-byte profile key + local display name.
#[derive(Debug, Clone)]
pub struct OwnProfile {
    pub profile_key: Vec<u8>,
    pub display_name: String,
}

/// A cached contact profile.
#[derive(Debug, Clone)]
pub struct ContactProfile {
    pub did: String,
    pub display_name: String,
    pub profile_key: Vec<u8>,
    pub fetched_at: Timestamp,
}

impl Store {
    /// Persist (or replace) the user's own profile key and display name.
    pub async fn save_own_profile(&self, profile: &OwnProfile) -> Result<(), StoreError> {
        let pk = profile.profile_key.clone();
        let name = profile.display_name.clone();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO own_profile (id, profile_key, display_name)
                     VALUES (1, ?1, ?2)",
                    rusqlite::params![pk, name],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Load the user's own profile state. Returns `None` if not yet created.
    pub async fn load_own_profile(&self) -> Result<Option<OwnProfile>, StoreError> {
        self.conn
            .call(|conn| {
                conn.query_row(
                    "SELECT profile_key, display_name FROM own_profile WHERE id = 1",
                    [],
                    |row| {
                        Ok(OwnProfile {
                            profile_key: row.get::<_, Vec<u8>>(0)?,
                            display_name: row.get::<_, String>(1)?,
                        })
                    },
                )
                .optional()
                .map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Update just the local display name on the own profile row.
    pub async fn update_own_display_name(&self, name: &str) -> Result<(), StoreError> {
        let name = name.to_string();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "UPDATE own_profile SET display_name = ?1 WHERE id = 1",
                    rusqlite::params![name],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Insert or update a cached contact profile.
    pub async fn upsert_contact_profile(&self, profile: &ContactProfile) -> Result<(), StoreError> {
        let did = profile.did.clone();
        let name = profile.display_name.clone();
        let pk = profile.profile_key.clone();
        let fetched_at = profile.fetched_at.as_millis();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO contact_profiles
                       (did, display_name, profile_key, fetched_at)
                     VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![did, name, pk, fetched_at],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Look up a cached contact profile by DID.
    pub async fn load_contact_profile(
        &self,
        did: &str,
    ) -> Result<Option<ContactProfile>, StoreError> {
        let did_q = did.to_string();
        self.conn
            .call(move |conn| {
                conn.query_row(
                    "SELECT did, display_name, profile_key, fetched_at
                     FROM contact_profiles WHERE did = ?1",
                    rusqlite::params![did_q],
                    |row| {
                        Ok(ContactProfile {
                            did: row.get::<_, String>(0)?,
                            display_name: row.get::<_, String>(1)?,
                            profile_key: row.get::<_, Vec<u8>>(2)?,
                            fetched_at: Timestamp(row.get::<_, i64>(3)?),
                        })
                    },
                )
                .optional()
                .map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Return the stored profile key for a contact, if cached.
    /// Used by the inbound-message hot path to decide whether a freshly
    /// received `profile_key` needs a fetch.
    pub async fn load_contact_profile_key(&self, did: &str) -> Result<Option<Vec<u8>>, StoreError> {
        let did_q = did.to_string();
        self.conn
            .call(move |conn| {
                conn.query_row(
                    "SELECT profile_key FROM contact_profiles WHERE did = ?1",
                    rusqlite::params![did_q],
                    |row| row.get::<_, Vec<u8>>(0),
                )
                .optional()
                .map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)
    }
}
