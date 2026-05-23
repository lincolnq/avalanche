//! Persistent push notification state for this device.
//!
//! Stores the current pseudonym, device token, and platform so they survive
//! app restarts. Only one row ever exists (id = 1).

use crate::{db::Store, error::StoreError};

/// Push registration state for the local device.
pub struct PushState {
    pub pseudonym: String,
    pub device_token: String,
    pub platform: String,
}

impl Store {
    /// Persist the current push registration. Replaces any prior state.
    pub async fn save_push_state(&self, state: PushState) -> Result<(), StoreError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO push_state \
                     (id, pseudonym, device_token, platform, registered_at) \
                     VALUES (1, ?1, ?2, ?3, ?4)",
                    rusqlite::params![state.pseudonym, state.device_token, state.platform, now],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Load the current push registration state. Returns `None` if not yet registered.
    pub async fn load_push_state(&self) -> Result<Option<PushState>, StoreError> {
        use rusqlite::OptionalExtension as _;
        self.conn
            .call(|conn| {
                conn.query_row(
                    "SELECT pseudonym, device_token, platform FROM push_state WHERE id = 1",
                    [],
                    |row| {
                        Ok(PushState {
                            pseudonym: row.get(0)?,
                            device_token: row.get(1)?,
                            platform: row.get(2)?,
                        })
                    },
                )
                .optional()
                .map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)
    }
}
