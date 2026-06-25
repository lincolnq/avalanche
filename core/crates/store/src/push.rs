//! Persistent push notification state for this device.
//!
//! Stores the current pseudonym, device token, and platform so they survive
//! app restarts. Only one row ever exists (id = 1).

use crate::{db::DeviceStore, error::StoreError};

/// Push registration state for the local device.
pub struct PushState {
    pub pseudonym: String,
    pub device_token: String,
    pub platform: String,
    /// Unix seconds when this registration was created. Used by the client
    /// to decide whether the pseudonym is due for rotation.
    pub registered_at: i64,
}

impl DeviceStore {
    /// Persist the current push registration. Replaces any prior state.
    /// The `registered_at` field on `state` is ignored — the current time
    /// is always written, since save = fresh registration.
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

    /// Remove the current push registration state (e.g. on logout). A no-op if
    /// no row exists. After this, [`Self::load_push_state`] returns `None` and
    /// the next `register_push_token` mints a fresh pseudonym.
    pub async fn clear_push_state(&self) -> Result<(), StoreError> {
        self.conn
            .call(move |conn| {
                conn.execute("DELETE FROM push_state WHERE id = 1", [])?;
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
                    "SELECT pseudonym, device_token, platform, registered_at \
                     FROM push_state WHERE id = 1",
                    [],
                    |row| {
                        Ok(PushState {
                            pseudonym: row.get(0)?,
                            device_token: row.get(1)?,
                            platform: row.get(2)?,
                            registered_at: row.get(3)?,
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
