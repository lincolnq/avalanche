use rusqlite::OptionalExtension as _;

use crate::{db::Store, error::StoreError};

/// A `conversation_settings` row as stored. Distinct from
/// [`Store::load_conversation_expiry`], which collapses "no row" and "timer
/// disabled" into the same `None`: the storage-sync engine needs to tell those
/// apart so it pushes a real record (row exists, no timer) rather than a
/// tombstone (row absent). `expiry_secs` is the raw column (`None` = NULL/0).
#[derive(Debug, Clone)]
pub struct ConversationSettingsRow {
    pub conversation_id: String,
    pub expiry_secs: Option<u32>,
}

impl Store {
    pub async fn save_conversation_expiry(
        &self,
        conversation_id: &str,
        expiry_secs: Option<u32>,
    ) -> Result<(), StoreError> {
        let cid = conversation_id.to_string();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO conversation_settings \
                     (conversation_id, expiry_secs) VALUES (?1, ?2)",
                    rusqlite::params![cid, expiry_secs.map(|v| v as i64)],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    pub async fn load_conversation_expiry(
        &self,
        conversation_id: &str,
    ) -> Result<Option<u32>, StoreError> {
        let cid = conversation_id.to_string();
        self.conn
            .call(move |conn| {
                // get::<_, Option<i64>> handles SQL NULL correctly.
                let row: Option<Option<i64>> = conn
                    .query_row(
                        "SELECT expiry_secs FROM conversation_settings \
                         WHERE conversation_id = ?1",
                        rusqlite::params![cid],
                        |row| row.get::<_, Option<i64>>(0),
                    )
                    .optional()?;
                // None     → no row     → no timer set
                // Some(None) → NULL column → no timer set
                // Some(Some(v)) → v > 0 is a timer; v = 0 means "disabled"
                Ok(row.flatten().and_then(|v| if v > 0 { Some(v as u32) } else { None }))
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Load the raw `conversation_settings` row, distinguishing "no row" (`None`)
    /// from "row present with no/zero timer" (`Some(row)` with `expiry_secs:
    /// None`). Used by the storage-sync engine's read path (docs/05).
    pub async fn load_conversation_settings(
        &self,
        conversation_id: &str,
    ) -> Result<Option<ConversationSettingsRow>, StoreError> {
        let cid = conversation_id.to_string();
        self.conn
            .call(move |conn| {
                conn.query_row(
                    "SELECT conversation_id, expiry_secs FROM conversation_settings \
                     WHERE conversation_id = ?1",
                    rusqlite::params![cid],
                    |row| {
                        let raw: Option<i64> = row.get(1)?;
                        Ok(ConversationSettingsRow {
                            conversation_id: row.get::<_, String>(0)?,
                            // Treat NULL and 0 alike as "no timer", matching
                            // load_conversation_expiry's interpretation.
                            expiry_secs: raw.and_then(|v| if v > 0 { Some(v as u32) } else { None }),
                        })
                    },
                )
                .optional()
                .map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Remove a `conversation_settings` row. Used by the storage-sync engine to
    /// apply a pulled tombstone (docs/05).
    pub async fn delete_conversation_settings(
        &self,
        conversation_id: &str,
    ) -> Result<(), StoreError> {
        let cid = conversation_id.to_string();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "DELETE FROM conversation_settings WHERE conversation_id = ?1",
                    rusqlite::params![cid],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }
}
