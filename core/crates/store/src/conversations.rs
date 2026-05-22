use rusqlite::OptionalExtension as _;
use crate::{db::Store, error::StoreError};

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
                    "INSERT OR REPLACE INTO conversation_settings (conversation_id, expiry_secs) VALUES (?1, ?2)",
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
                let result: Option<i64> = conn
                    .query_row(
                        "SELECT expiry_secs FROM conversation_settings WHERE conversation_id = ?1",
                        rusqlite::params![cid],
                        |row| row.get(0),
                    )
                    .optional()?;
                Ok(result.and_then(|v| if v >= 0 { Some(v as u32) } else { None }))
            })
            .await
            .map_err(StoreError::Db)
    }
}
