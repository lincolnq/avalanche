//! Outbound message queue and message history.
//!
//! Two responsibilities:
//!
//! 1. **Outbound queue** — encrypted messages pending delivery. Plaintext never
//!    touches the queue; it holds ciphertext only.
//!
//! 2. **Message history** — decrypted messages persisted for chat continuity
//!    across app restarts. Stored encrypted-at-rest by SQLCipher.

use rusqlite::OptionalExtension as _;
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

    /// Enumerate every known conversation, with the most recent message
    /// attached if any. One row per `conversation_id`, sorted newest-first
    /// (conversations with no messages sort to the end by `created_at` of
    /// the underlying group). The chat list is built directly from these
    /// rows — the mobile layer keeps no parallel store.
    ///
    /// Includes:
    /// - Every conversation that has at least one persisted message
    ///   (DM peers and groups alike).
    /// - Every group we know about (master key persisted via
    ///   `store_inbound_group_context` or `create_group`), even if no
    ///   messages have arrived yet, so a fresh invite is visible.
    pub async fn load_conversations(&self) -> Result<Vec<ConversationSummary>, StoreError> {
        self.conn
            .call(|conn| {
                // 1. Latest message per conversation that has any messages.
                let mut stmt = conn.prepare(
                    "SELECT m.conversation_id, m.id, m.sender_did, m.body, m.sent_at,
                            m.edited_at, m.read_at, m.delivery_status
                     FROM message_history m
                     JOIN (
                         SELECT conversation_id, MAX(sent_at) AS max_sent
                         FROM message_history
                         GROUP BY conversation_id
                     ) latest
                       ON m.conversation_id = latest.conversation_id
                      AND m.sent_at = latest.max_sent
                     ORDER BY m.sent_at DESC",
                )?;
                let with_msgs: Vec<ConversationSummary> = stmt
                    .query_map([], |row| {
                        Ok(ConversationSummary {
                            conversation_id: row.get(0)?,
                            last_message: Some(HistoryMessage {
                                id: row.get(1)?,
                                conversation_id: row.get(0)?,
                                sender_did: row.get(2)?,
                                body: row.get(3)?,
                                sent_at: Timestamp(row.get(4)?),
                                edited_at: row.get::<_, Option<i64>>(5)?.map(Timestamp),
                                read_at: row.get::<_, Option<i64>>(6)?.map(Timestamp),
                                delivery_status: row.get::<_, i64>(7)? as u8,
                            }),
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?;

                // 2. Groups that exist locally but aren't yet represented in
                //    message_history. The conversation_id for a group is
                //    `group-<groupId>` — the same prefix the mobile layer
                //    uses when it writes group messages into history (see
                //    `groupConversationId` in the iOS sources), so a row
                //    here lines up with what arrives once any message
                //    lands.
                let known: std::collections::HashSet<String> =
                    with_msgs.iter().map(|c| c.conversation_id.clone()).collect();
                let mut group_stmt = conn.prepare(
                    "SELECT group_id FROM groups ORDER BY created_at ASC",
                )?;
                let empty_groups: Vec<ConversationSummary> = group_stmt
                    .query_map([], |row| row.get::<_, String>(0))?
                    .filter_map(|r| r.ok())
                    .map(|gid| format!("group-{gid}"))
                    .filter(|cid| !known.contains(cid))
                    .map(|cid| ConversationSummary {
                        conversation_id: cid,
                        last_message: None,
                    })
                    .collect();

                let mut out = with_msgs;
                out.extend(empty_groups);
                Ok(out)
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Load just the most recent message for a conversation. Returns `None`
    /// if the conversation has no messages. Used for restoring conversation
    /// list previews after app restart (the plaintext body lives only here,
    /// not in UserDefaults).
    pub async fn load_last_message(
        &self,
        conversation_id: &str,
    ) -> Result<Option<HistoryMessage>, StoreError> {
        let conv_id = conversation_id.to_string();
        self.conn
            .call(move |conn| {
                conn.query_row(
                    "SELECT id, conversation_id, sender_did, body, sent_at, edited_at, read_at, delivery_status
                     FROM message_history
                     WHERE conversation_id = ?1
                     ORDER BY sent_at DESC
                     LIMIT 1",
                    [&conv_id],
                    |row| {
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
                    },
                )
                .optional()
                .map_err(Into::into)
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

/// One row per conversation: the conversation identifier plus the most
/// recent message in it, if any. `last_message: None` rows are conversations
/// known to the local store (e.g. groups we've been invited to) that don't
/// yet have any persisted messages. The chat list is built directly from
/// these rows.
#[derive(Debug, Clone)]
pub struct ConversationSummary {
    pub conversation_id: String,
    pub last_message: Option<HistoryMessage>,
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
