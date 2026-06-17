//! Minimal contact table backing the People list and compose autocomplete.
//!
//! See `docs/52-contacts-and-profiles.md` for the full design. This module
//! implements the slice in use today: per-DID `is_curated` (true after any
//! deliberate gesture — sending a DM, inviting to a group), `is_blocked`
//! (docs/12 §2 — suppresses the DID; synced across devices), a local-only
//! `has_pending_request` (an un-accepted inbound first contact), and
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
    pub is_blocked: bool,
    pub has_pending_request: bool,
}

impl IdentityStore {
    /// Touch a contact row, creating it if missing. `curated_now` flips
    /// `is_curated` to true on any deliberate gesture; passing false leaves
    /// the existing value intact (sticky). `interaction_at` updates only if
    /// it's newer than the stored value. Never touches `is_blocked` or
    /// `has_pending_request`.
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

    /// Set or clear the block flag for a DID, creating a bare row if missing
    /// (docs/12 §2: blocking a never-seen DID creates a row with only
    /// `is_blocked` set). Does not change `is_curated` — a relationship that
    /// existed before a block stays remembered (docs/52 §"What is_curated
    /// drives"). The local write trips the storage-sync trigger so the block
    /// propagates to the identity's other devices.
    pub async fn set_blocked(&self, did: &str, blocked: bool) -> Result<(), StoreError> {
        let did_s = did.to_string();
        let b = if blocked { 1 } else { 0 };
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO contacts (did, is_blocked)
                     VALUES (?1, ?2)
                     ON CONFLICT(did) DO UPDATE SET is_blocked = excluded.is_blocked",
                    rusqlite::params![did_s, b],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Set or clear the local `has_pending_request` flag, creating a bare row
    /// if missing. Set on an inbound first contact from an un-curated DID;
    /// cleared on accept / delete / send / block / report. Local-only — not
    /// carried by the contact sync adapter.
    pub async fn set_pending_request(&self, did: &str, pending: bool) -> Result<(), StoreError> {
        let did_s = did.to_string();
        let p = if pending { 1 } else { 0 };
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO contacts (did, has_pending_request)
                     VALUES (?1, ?2)
                     ON CONFLICT(did) DO UPDATE SET has_pending_request = excluded.has_pending_request",
                    rusqlite::params![did_s, p],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Apply a contact record pulled from the storage-sync service (docs/05).
    /// The engine only ever applies strictly-newer record versions (LWW), so:
    /// `is_curated` and `last_interaction_at` take a monotonic `MAX` (both are
    /// monotonic and a re-applied older value must not rewind them), while
    /// `is_blocked` is overwritten with the authoritative pulled value so an
    /// unblock on another device propagates.
    pub async fn apply_synced_contact(
        &self,
        did: &str,
        is_curated: bool,
        is_blocked: bool,
        last_interaction_at: Timestamp,
    ) -> Result<(), StoreError> {
        let did_s = did.to_string();
        let curated = if is_curated { 1 } else { 0 };
        let blocked = if is_blocked { 1 } else { 0 };
        let ts = last_interaction_at.as_millis();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO contacts (did, is_curated, last_interaction_at, is_blocked)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(did) DO UPDATE SET
                       is_curated = MAX(is_curated, excluded.is_curated),
                       last_interaction_at = MAX(last_interaction_at, excluded.last_interaction_at),
                       is_blocked = excluded.is_blocked",
                    rusqlite::params![did_s, curated, ts, blocked],
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
                    "SELECT did, is_curated, last_interaction_at, is_blocked, has_pending_request
                     FROM contacts WHERE did = ?1",
                    rusqlite::params![did_q],
                    row_to_contact,
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
                    "SELECT did, is_curated, last_interaction_at, is_blocked, has_pending_request
                     FROM contacts
                     ORDER BY last_interaction_at DESC, did ASC",
                )?;
                let rows = stmt.query_map([], row_to_contact)?;
                rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)
    }

    /// The block list: every row where `is_blocked`, newest interaction first.
    /// Backs Settings → Privacy → Blocked (docs/12 §7).
    pub async fn list_blocked(&self) -> Result<Vec<ContactRow>, StoreError> {
        self.conn
            .call(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT did, is_curated, last_interaction_at, is_blocked, has_pending_request
                     FROM contacts
                     WHERE is_blocked = 1
                     ORDER BY last_interaction_at DESC, did ASC",
                )?;
                let rows = stmt.query_map([], row_to_contact)?;
                rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)
    }
}

fn row_to_contact(row: &rusqlite::Row<'_>) -> rusqlite::Result<ContactRow> {
    Ok(ContactRow {
        did: row.get::<_, String>(0)?,
        is_curated: row.get::<_, i64>(1)? != 0,
        last_interaction_at: Timestamp(row.get::<_, i64>(2)?),
        is_blocked: row.get::<_, i64>(3)? != 0,
        has_pending_request: row.get::<_, i64>(4)? != 0,
    })
}
