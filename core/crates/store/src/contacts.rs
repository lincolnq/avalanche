//! Minimal contact table backing the People list and compose autocomplete.
//!
//! See `docs/52-contacts-and-profiles.md` for the full design. This module
//! implements the smallest useful slice: per-DID `is_curated` flag (true after
//! any deliberate gesture — sending a DM, inviting to a group) and
//! `last_interaction_at` for recency sort. Display-name resolution still flows
//! through [`crate::profiles`].

use rusqlite::OptionalExtension as _;
use types::Timestamp;

use crate::{db::IdentityStore, error::StoreError};

#[derive(Debug, Clone)]
pub struct ContactRow {
    pub did: String,
    pub is_curated: bool,
    pub last_interaction_at: Timestamp,
}

impl IdentityStore {
    /// Touch a contact row, creating it if missing. `curated_now` flips
    /// `is_curated` to true on any deliberate gesture; passing false leaves
    /// the existing value intact (sticky). `interaction_at` updates only if
    /// it's newer than the stored value.
    pub async fn touch_contact(
        &self,
        did: &str,
        curated_now: bool,
        interaction_at: Timestamp,
    ) -> Result<(), StoreError> {
        let did_s = did.to_string();
        let ts = interaction_at.as_millis();
        let curated = if curated_now { 1 } else { 0 };
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO contacts (did, is_curated, last_interaction_at)
                     VALUES (?1, ?2, ?3)
                     ON CONFLICT(did) DO UPDATE SET
                       is_curated = MAX(is_curated, excluded.is_curated),
                       last_interaction_at = MAX(last_interaction_at, excluded.last_interaction_at)",
                    rusqlite::params![did_s, curated, ts],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    pub async fn load_contact(&self, did: &str) -> Result<Option<ContactRow>, StoreError> {
        let did_q = did.to_string();
        self.conn
            .call(move |conn| {
                conn.query_row(
                    "SELECT did, is_curated, last_interaction_at FROM contacts WHERE did = ?1",
                    rusqlite::params![did_q],
                    |row| {
                        Ok(ContactRow {
                            did: row.get::<_, String>(0)?,
                            is_curated: row.get::<_, i64>(1)? != 0,
                            last_interaction_at: Timestamp(row.get::<_, i64>(2)?),
                        })
                    },
                )
                .optional()
                .map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Remove a contact row. Used by the storage-sync engine to apply a pulled
    /// tombstone (docs/05); the AFTER DELETE trigger marks the sidecar so the
    /// deletion also propagates to this device's other accounts.
    pub async fn delete_contact(&self, did: &str) -> Result<(), StoreError> {
        let did_s = did.to_string();
        self.conn
            .call(move |conn| {
                conn.execute("DELETE FROM contacts WHERE did = ?1", rusqlite::params![did_s])?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    /// List every known contact, newest interaction first. Caller filters
    /// for `is_curated` if it wants the People list.
    pub async fn list_contacts(&self) -> Result<Vec<ContactRow>, StoreError> {
        self.conn
            .call(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT did, is_curated, last_interaction_at
                     FROM contacts
                     ORDER BY last_interaction_at DESC, did ASC",
                )?;
                let rows = stmt.query_map([], |row| {
                    Ok(ContactRow {
                        did: row.get::<_, String>(0)?,
                        is_curated: row.get::<_, i64>(1)? != 0,
                        last_interaction_at: Timestamp(row.get::<_, i64>(2)?),
                    })
                })?;
                rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)
    }
}
