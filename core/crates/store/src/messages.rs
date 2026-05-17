//! Outbound message queue and message history.
//!
//! Two responsibilities:
//!
//! 1. **Outbound queue** — encrypted messages pending delivery. Plaintext never
//!    touches the queue; it holds ciphertext only.
//!
//! 2. **Message history** — decrypted messages persisted for chat continuity
//!    across app restarts. Stored encrypted-at-rest by SQLCipher.

use types::{MessageId, Timestamp};

use crate::{db::Store, error::StoreError};

/// An encrypted message held in the outbound queue pending delivery.
#[derive(Debug, Clone)]
pub struct QueuedMessage {
    pub id: MessageId,
    pub recipient_name: String,
    pub recipient_device_id: u32,
    pub ciphertext: Vec<u8>,
    /// 0 = PreKey, 1 = Whisper
    pub message_kind: u8,
    pub enqueued_at: Timestamp,
}

impl Store {
    /// Add a message to the outbound queue.
    pub async fn enqueue(&self, msg: &QueuedMessage) -> Result<(), StoreError> {
        let id = msg.id.to_string();
        let recipient_name = msg.recipient_name.clone();
        let recipient_device_id = msg.recipient_device_id;
        let ciphertext = msg.ciphertext.clone();
        let message_kind = msg.message_kind as i64;
        let enqueued_at = msg.enqueued_at.as_millis();

        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO message_queue
                     (id, recipient_name, recipient_device_id, ciphertext, message_kind, enqueued_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![
                        id,
                        recipient_name,
                        recipient_device_id,
                        ciphertext,
                        message_kind,
                        enqueued_at
                    ],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Return all queued messages, oldest first.
    pub async fn drain(&self) -> Result<Vec<QueuedMessage>, StoreError> {
        self.conn
            .call(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, recipient_name, recipient_device_id, ciphertext,
                            message_kind, enqueued_at
                     FROM message_queue
                     ORDER BY enqueued_at ASC",
                )?;

                let rows = stmt.query_map([], |row| {
                    Ok(QueuedMessage {
                        id: MessageId(
                            uuid::Uuid::parse_str(&row.get::<_, String>(0)?)
                                .map_err(|e| rusqlite::Error::FromSqlConversionFailure(
                                    0,
                                    rusqlite::types::Type::Text,
                                    Box::new(e),
                                ))?,
                        ),
                        recipient_name: row.get(1)?,
                        recipient_device_id: row.get(2)?,
                        ciphertext: row.get(3)?,
                        message_kind: row.get::<_, i64>(4)? as u8,
                        enqueued_at: Timestamp(row.get(5)?),
                    })
                })?;

                rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Remove a delivered message from the queue.
    pub async fn mark_delivered(&self, id: MessageId) -> Result<(), StoreError> {
        let id_str = id.to_string();
        self.conn
            .call(move |conn| {
                conn.execute("DELETE FROM message_queue WHERE id = ?1", [&id_str])?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    // ── Message history ─────────────────────────────────────────────────

    /// Save a decrypted message to the local history.
    pub async fn save_message(&self, msg: &HistoryMessage) -> Result<(), StoreError> {
        let id = msg.id.clone();
        let conversation_id = msg.conversation_id.clone();
        let sender_did = msg.sender_did.clone();
        let body = msg.body.clone();
        let sent_at = msg.sent_at.as_millis();
        let edited_at = msg.edited_at.map(|t| t.as_millis());
        let read_at = msg.read_at.map(|t| t.as_millis());
        let delivery_status = msg.delivery_status as i64;

        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO message_history
                     (id, conversation_id, sender_did, body, sent_at, edited_at, read_at, delivery_status)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    rusqlite::params![id, conversation_id, sender_did, body, sent_at, edited_at, read_at, delivery_status],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Load messages for a conversation, ordered by sent_at ascending.
    pub async fn load_messages(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<HistoryMessage>, StoreError> {
        let conv_id = conversation_id.to_string();
        self.conn
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, conversation_id, sender_did, body, sent_at, edited_at, read_at, delivery_status
                     FROM message_history
                     WHERE conversation_id = ?1
                     ORDER BY sent_at ASC",
                )?;
                let rows = stmt.query_map([&conv_id], |row| {
                    Ok(HistoryMessage {
                        id: row.get(0)?,
                        conversation_id: row.get(1)?,
                        sender_did: row.get(2)?,
                        body: row.get(3)?,
                        sent_at: Timestamp(row.get(4)?),
                        edited_at: row.get::<_, Option<i64>>(5)?.map(Timestamp),
                        read_at: row.get::<_, Option<i64>>(6)?.map(Timestamp),
                        delivery_status: row.get::<_, i64>(7)? as u8,
                    })
                })?;
                rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Mark all unread messages in a conversation as read up to a given timestamp.
    /// Sets `read_at` to `now` for matching messages. Returns the number of messages marked.
    pub async fn mark_messages_read(
        &self,
        conversation_id: &str,
        up_to_sent_at: Timestamp,
        now: Timestamp,
    ) -> Result<u64, StoreError> {
        let conv_id = conversation_id.to_string();
        let up_to = up_to_sent_at.as_millis();
        let read_at = now.as_millis();

        self.conn
            .call(move |conn| {
                let count = conn.execute(
                    "UPDATE message_history
                     SET read_at = ?1
                     WHERE conversation_id = ?2 AND sent_at <= ?3 AND read_at IS NULL",
                    rusqlite::params![read_at, conv_id, up_to],
                )?;
                Ok(count as u64)
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Update delivery_status for outgoing messages matching the given sent_at timestamps.
    /// Returns the number of messages updated.
    pub async fn update_delivery_status(
        &self,
        conversation_id: &str,
        timestamps: &[i64],
        new_status: u8,
    ) -> Result<u64, StoreError> {
        let conv_id = conversation_id.to_string();
        let ts = timestamps.to_vec();
        let status = new_status as i64;

        self.conn
            .call(move |conn| {
                let mut total = 0u64;
                for sent_at in &ts {
                    let count = conn.execute(
                        "UPDATE message_history
                         SET delivery_status = ?1
                         WHERE conversation_id = ?2 AND sent_at = ?3 AND delivery_status < ?1",
                        rusqlite::params![status, conv_id, sent_at],
                    )?;
                    total += count as u64;
                }
                Ok(total)
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Count unread messages in a conversation (messages not sent by own_did with read_at IS NULL).
    pub async fn unread_count(
        &self,
        conversation_id: &str,
        own_did: &str,
    ) -> Result<u64, StoreError> {
        let conv_id = conversation_id.to_string();
        let did = own_did.to_string();

        self.conn
            .call(move |conn| {
                let count: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM message_history
                     WHERE conversation_id = ?1 AND read_at IS NULL AND sender_did != ?2",
                    rusqlite::params![conv_id, did],
                    |row| row.get(0),
                )?;
                Ok(count as u64)
            })
            .await
            .map_err(StoreError::Db)
    }
}

/// A decrypted message stored in the local history.
#[derive(Debug, Clone)]
pub struct HistoryMessage {
    pub id: String,
    pub conversation_id: String,
    pub sender_did: String,
    pub body: String,
    pub sent_at: Timestamp,
    pub edited_at: Option<Timestamp>,
    /// NULL = unread, Some = unix millis when marked read.
    pub read_at: Option<Timestamp>,
    /// 0 = sending, 1 = sent, 2 = delivered, 3 = read.
    pub delivery_status: u8,
}
