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

use crate::{
    db::{DeviceStore, IdentityStore},
    error::StoreError,
};

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

impl DeviceStore {
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

}

impl IdentityStore {
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
        let expire_timer_secs = msg.expire_timer_secs;
        // Disappearing-messages deadline (docs/03 §5): the countdown starts when
        // the message is read, so compute `expire_at` only once `read_at` is set
        // and a timer applies. Outgoing rows are saved read (`read_at = sent_at`)
        // so they get a deadline here; incoming rows start NULL and get theirs in
        // `mark_messages_read`.
        let expire_at: Option<i64> = if expire_timer_secs > 0 {
            read_at.map(|r| r + expire_timer_secs * 1000)
        } else {
            None
        };

        self.conn
            .call(move |conn| {
                // UPSERT (not INSERT OR REPLACE) so the Rust-managed columns
                // edit_count and deleted_at survive a re-save of an existing
                // row (e.g. an outgoing message's status transitions). Those
                // columns are owned by apply_edit / tombstone_message, never by
                // the caller of save_message.
                // Normal chat messages are kind=0 with no metadata; system
                // timeline entries go through `save_group_event` instead. We
                // don't touch kind/metadata/expire_* in the conflict clause so a
                // re-save of an existing row (status transitions) can't downgrade
                // a system row to kind 0 or recompute a started countdown.
                conn.execute(
                    "INSERT INTO message_history
                     (id, conversation_id, sender_did, body, sent_at, edited_at, read_at, delivery_status, kind, metadata, expire_timer_secs, expire_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, NULL, ?9, ?10)
                     ON CONFLICT(id) DO UPDATE SET
                         conversation_id = excluded.conversation_id,
                         sender_did      = excluded.sender_did,
                         body            = excluded.body,
                         sent_at         = excluded.sent_at,
                         edited_at       = excluded.edited_at,
                         read_at         = excluded.read_at,
                         delivery_status = excluded.delivery_status",
                    rusqlite::params![id, conversation_id, sender_did, body, sent_at, edited_at, read_at, delivery_status, expire_timer_secs, expire_at],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Persist a system/metadata timeline entry (docs/03 §3.6 group events).
    /// Idempotent: re-applying the same `/changes` page is a no-op because the
    /// deterministic `id` (`grpevt-<group>-<revision>-<seq>`) collides and we
    /// `DO NOTHING`. System rows are written read (`read_at = sent_at`) so they
    /// never inflate the unread count, and carry `kind`/`metadata`.
    pub async fn save_group_event(&self, msg: &HistoryMessage) -> Result<(), StoreError> {
        let id = msg.id.clone();
        let conversation_id = msg.conversation_id.clone();
        let sender_did = msg.sender_did.clone();
        let body = msg.body.clone();
        let sent_at = msg.sent_at.as_millis();
        let kind = msg.kind;
        let metadata = msg.metadata.clone();

        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO message_history
                     (id, conversation_id, sender_did, body, sent_at, read_at, delivery_status, kind, metadata)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?5, 1, ?6, ?7)
                     ON CONFLICT(id) DO NOTHING",
                    rusqlite::params![id, conversation_id, sender_did, body, sent_at, kind, metadata],
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
        now: Timestamp,
    ) -> Result<Vec<HistoryMessage>, StoreError> {
        let conv_id = conversation_id.to_string();
        let now_ms = now.as_millis();
        self.conn
            .call(move |conn| {
                // Exclude messages past their disappearing-messages deadline
                // (docs/03 §5): "expired" is invisible immediately, independent
                // of when the background reaper physically deletes the row.
                let mut stmt = conn.prepare(
                    "SELECT id, conversation_id, sender_did, body, sent_at, edited_at, read_at, delivery_status, edit_count, deleted_at, kind, metadata, expire_timer_secs, expire_at
                     FROM message_history
                     WHERE conversation_id = ?1 AND (expire_at IS NULL OR expire_at > ?2)
                     ORDER BY sent_at ASC",
                )?;
                let rows = stmt.query_map(rusqlite::params![&conv_id, now_ms], |row| {
                    Ok(HistoryMessage {
                        id: row.get(0)?,
                        conversation_id: row.get(1)?,
                        sender_did: row.get(2)?,
                        body: row.get(3)?,
                        sent_at: Timestamp(row.get(4)?),
                        edited_at: row.get::<_, Option<i64>>(5)?.map(Timestamp),
                        read_at: row.get::<_, Option<i64>>(6)?.map(Timestamp),
                        delivery_status: row.get::<_, i64>(7)? as u8,
                        edit_count: row.get::<_, i64>(8)? as u32,
                        deleted_at: row.get::<_, Option<i64>>(9)?.map(Timestamp),
                        kind: row.get::<_, i64>(10)?,
                        metadata: row.get::<_, Option<String>>(11)?,
                        expire_timer_secs: row.get::<_, i64>(12)?,
                        expire_at: row.get::<_, Option<i64>>(13)?.map(Timestamp),
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
    pub async fn load_conversations(&self, now: Timestamp) -> Result<Vec<ConversationSummary>, StoreError> {
        let now_ms = now.as_millis();
        self.conn
            .call(move |conn| {
                // 1. Latest non-expired message per conversation. Expired
                //    messages (docs/03 §5) are excluded so a disappeared message
                //    never surfaces as a preview, regardless of reaper timing.
                //
                //    Use ROW_NUMBER() to pick exactly ONE row per conversation.
                //    A plain MAX(sent_at) self-join returns one row *per tie*, so
                //    a conversation with several messages sharing the max
                //    timestamp (e.g. a burst of group membership events / SKDMs
                //    delivered to a freshly linked device, some with identical or
                //    zero sent_at) would surface as duplicate conversation rows.
                //    Tie-break on `id` (monotonic rowid) for a deterministic pick.
                let mut stmt = conn.prepare(
                    "SELECT conversation_id, id, sender_did, body, sent_at,
                            edited_at, read_at, delivery_status, edit_count, deleted_at, kind, metadata, expire_timer_secs, expire_at
                     FROM (
                         SELECT m.conversation_id, m.id, m.sender_did, m.body, m.sent_at,
                                m.edited_at, m.read_at, m.delivery_status, m.edit_count, m.deleted_at, m.kind, m.metadata, m.expire_timer_secs, m.expire_at,
                                ROW_NUMBER() OVER (
                                    PARTITION BY m.conversation_id
                                    ORDER BY m.sent_at DESC, m.id DESC
                                ) AS rn
                         FROM message_history m
                         WHERE m.expire_at IS NULL OR m.expire_at > ?1
                     )
                     WHERE rn = 1
                     ORDER BY sent_at DESC",
                )?;
                let with_msgs: Vec<ConversationSummary> = stmt
                    .query_map([now_ms], |row| {
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
                                edit_count: row.get::<_, i64>(8)? as u32,
                                deleted_at: row.get::<_, Option<i64>>(9)?.map(Timestamp),
                                kind: row.get::<_, i64>(10)?,
                                metadata: row.get::<_, Option<String>>(11)?,
                            expire_timer_secs: row.get::<_, i64>(12)?,
                            expire_at: row.get::<_, Option<i64>>(13)?.map(Timestamp),
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
        now: Timestamp,
    ) -> Result<Option<HistoryMessage>, StoreError> {
        let conv_id = conversation_id.to_string();
        let now_ms = now.as_millis();
        self.conn
            .call(move |conn| {
                conn.query_row(
                    "SELECT id, conversation_id, sender_did, body, sent_at, edited_at, read_at, delivery_status, edit_count, deleted_at, kind, metadata, expire_timer_secs, expire_at
                     FROM message_history
                     WHERE conversation_id = ?1 AND (expire_at IS NULL OR expire_at > ?2)
                     ORDER BY sent_at DESC
                     LIMIT 1",
                    rusqlite::params![&conv_id, now_ms],
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
                            edit_count: row.get::<_, i64>(8)? as u32,
                            deleted_at: row.get::<_, Option<i64>>(9)?.map(Timestamp),
                            kind: row.get::<_, i64>(10)?,
                            metadata: row.get::<_, Option<String>>(11)?,
                            expire_timer_secs: row.get::<_, i64>(12)?,
                            expire_at: row.get::<_, Option<i64>>(13)?.map(Timestamp),
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
                     SET read_at = ?1,
                         -- Start the disappearing-messages countdown on read
                         -- (docs/03 §5): set the deadline for timed messages
                         -- that haven't started one yet.
                         expire_at = CASE
                             WHEN expire_timer_secs > 0 AND expire_at IS NULL
                             THEN ?1 + expire_timer_secs * 1000
                             ELSE expire_at
                         END
                     WHERE conversation_id = ?2 AND sent_at <= ?3 AND read_at IS NULL",
                    rusqlite::params![read_at, conv_id, up_to],
                )?;
                Ok(count as u64)
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Hard-delete every message whose disappearing-messages deadline has passed
    /// (docs/03 §5), returning the distinct conversation ids that lost a row so
    /// the UI can refresh them. Unlike a user delete this leaves no tombstone —
    /// the row simply ceases to exist. `now` is unix millis.
    pub async fn delete_expired_messages(&self, now: Timestamp) -> Result<Vec<String>, StoreError> {
        let now_ms = now.as_millis();
        self.conn
            .call(move |conn| {
                // Collect affected conversations first, then delete (broad
                // SQLite support — avoids relying on DELETE ... RETURNING).
                let mut stmt = conn.prepare(
                    "SELECT DISTINCT conversation_id FROM message_history
                     WHERE expire_at IS NOT NULL AND expire_at <= ?1",
                )?;
                let convs: Vec<String> = stmt
                    .query_map([now_ms], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                conn.execute(
                    "DELETE FROM message_history WHERE expire_at IS NOT NULL AND expire_at <= ?1",
                    [now_ms],
                )?;
                Ok(convs)
            })
            .await
            .map_err(StoreError::Db)
    }

    /// The soonest disappearing-messages deadline still pending (docs/03 §5),
    /// or `None` if no timed message has a started countdown. The background
    /// reaper uses this to schedule its next wakeup precisely.
    pub async fn next_expire_at(&self) -> Result<Option<i64>, StoreError> {
        self.conn
            .call(|conn| {
                conn.query_row(
                    "SELECT MIN(expire_at) FROM message_history WHERE expire_at IS NOT NULL",
                    [],
                    |row| row.get::<_, Option<i64>>(0),
                )
                .optional()
                .map(|opt| opt.flatten())
                .map_err(Into::into)
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

    // ── Editing & deletion (docs/36-message-editing-deletion.md) ──────────

    /// Find a message by its wire identity `(conversation_id, author, sent_at)`.
    /// Used to verify a target exists and to read its `edit_count` before
    /// enforcing the human edit cap on send.
    pub async fn find_message(
        &self,
        conversation_id: &str,
        author: &str,
        sent_at: Timestamp,
    ) -> Result<Option<HistoryMessage>, StoreError> {
        let conv = conversation_id.to_string();
        let author = author.to_string();
        let sent = sent_at.as_millis();
        self.conn
            .call(move |conn| {
                conn.query_row(
                    "SELECT id, conversation_id, sender_did, body, sent_at, edited_at, read_at, delivery_status, edit_count, deleted_at, kind, metadata, expire_timer_secs, expire_at
                     FROM message_history
                     WHERE conversation_id = ?1 AND sender_did = ?2 AND sent_at = ?3",
                    rusqlite::params![conv, author, sent],
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
                            edit_count: row.get::<_, i64>(8)? as u32,
                            deleted_at: row.get::<_, Option<i64>>(9)?.map(Timestamp),
                            kind: row.get::<_, i64>(10)?,
                            metadata: row.get::<_, Option<String>>(11)?,
                            expire_timer_secs: row.get::<_, i64>(12)?,
                            expire_at: row.get::<_, Option<i64>>(13)?.map(Timestamp),
                        })
                    },
                )
                .optional()
                .map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Apply an in-place edit to `(conversation_id, author, target_sent_at)`.
    /// Last-writer-wins on `edited_at`: an edit older than the one already
    /// applied is ignored. A tombstoned message absorbs edits (no-op). When
    /// `store_revision` is true the superseded body is pushed to
    /// `message_revisions` for the history sheet (skipped for bot authors).
    /// Returns true if the target exists (whether or not the edit was newer).
    pub async fn apply_edit(
        &self,
        conversation_id: &str,
        author: &str,
        target_sent_at: Timestamp,
        new_body: &str,
        edited_at: Timestamp,
        store_revision: bool,
    ) -> Result<bool, StoreError> {
        let conv = conversation_id.to_string();
        let author = author.to_string();
        let sent = target_sent_at.as_millis();
        let new_body = new_body.to_string();
        let edited = edited_at.as_millis();
        self.conn
            .call(move |conn| {
                let existing: Option<(String, Option<i64>, Option<i64>)> = conn
                    .query_row(
                        "SELECT body, edited_at, deleted_at FROM message_history
                         WHERE conversation_id = ?1 AND sender_did = ?2 AND sent_at = ?3",
                        rusqlite::params![conv, author, sent],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                    )
                    .optional()?;
                let Some((old_body, cur_edited, deleted_at)) = existing else {
                    // Target not yet arrived — out-of-order edit, dropped.
                    return Ok(false);
                };
                // Tombstone is absorbing; never un-delete via an edit.
                if deleted_at.is_some() {
                    return Ok(true);
                }
                // LWW: ignore an edit not newer than the applied one.
                if cur_edited.is_some_and(|c| edited <= c) {
                    return Ok(true);
                }
                if store_revision {
                    conn.execute(
                        "INSERT INTO message_revisions
                         (conversation_id, author_did, target_sent_at, body, replaced_at)
                         VALUES (?1, ?2, ?3, ?4, ?5)",
                        rusqlite::params![conv, author, sent, old_body, edited],
                    )?;
                }
                conn.execute(
                    "UPDATE message_history
                     SET body = ?1, edited_at = ?2, edit_count = edit_count + 1
                     WHERE conversation_id = ?3 AND sender_did = ?4 AND sent_at = ?5",
                    rusqlite::params![new_body, edited, conv, author, sent],
                )?;
                Ok(true)
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Tombstone a message FOR_EVERYONE: clear its body, mark `deleted_at`,
    /// and drop its reactions and revisions. If the target hasn't arrived yet,
    /// insert a tombstone placeholder so the delete is terminal/absorbing.
    /// Idempotent: re-tombstoning an already-deleted message is a no-op.
    pub async fn tombstone_message(
        &self,
        conversation_id: &str,
        target_author: &str,
        target_sent_at: Timestamp,
        deleted_at: Timestamp,
    ) -> Result<(), StoreError> {
        let conv = conversation_id.to_string();
        let author = target_author.to_string();
        let sent = target_sent_at.as_millis();
        let deleted = deleted_at.as_millis();
        self.conn
            .call(move |conn| {
                // Tombstone the row if present. A FOR_EVERYONE delete causally
                // follows its own message, so the target is essentially always
                // already stored; an out-of-order delete-before-receive is
                // dropped here (we still clear any early-arriving reactions
                // below). `deleted_at IS NULL` keeps it idempotent.
                conn.execute(
                    "UPDATE message_history
                     SET body = '', edited_at = NULL, deleted_at = ?1
                     WHERE conversation_id = ?2 AND sender_did = ?3 AND sent_at = ?4
                       AND deleted_at IS NULL",
                    rusqlite::params![deleted, conv, author, sent],
                )?;
                conn.execute(
                    "DELETE FROM reactions
                     WHERE conversation_id = ?1 AND target_author = ?2 AND target_sent_at = ?3",
                    rusqlite::params![conv, author, sent],
                )?;
                conn.execute(
                    "DELETE FROM message_revisions
                     WHERE conversation_id = ?1 AND author_did = ?2 AND target_sent_at = ?3",
                    rusqlite::params![conv, author, sent],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Delete a message FOR_ME: remove the row and its reactions/revisions from
    /// this device only. Any message, no authorship check (only mutates the
    /// local view).
    pub async fn delete_message_for_me(
        &self,
        conversation_id: &str,
        target_author: &str,
        target_sent_at: Timestamp,
    ) -> Result<(), StoreError> {
        let conv = conversation_id.to_string();
        let author = target_author.to_string();
        let sent = target_sent_at.as_millis();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "DELETE FROM message_history
                     WHERE conversation_id = ?1 AND sender_did = ?2 AND sent_at = ?3",
                    rusqlite::params![conv, author, sent],
                )?;
                conn.execute(
                    "DELETE FROM reactions
                     WHERE conversation_id = ?1 AND target_author = ?2 AND target_sent_at = ?3",
                    rusqlite::params![conv, author, sent],
                )?;
                conn.execute(
                    "DELETE FROM message_revisions
                     WHERE conversation_id = ?1 AND author_did = ?2 AND target_sent_at = ?3",
                    rusqlite::params![conv, author, sent],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Delete an entire conversation's local history — every message plus its
    /// reactions and edit revisions. Local-only (this device's view); used by
    /// "Delete" on a message request (docs/12 §1). The contact row is left
    /// intact so a later inbound message starts a fresh request.
    pub async fn delete_conversation(&self, conversation_id: &str) -> Result<(), StoreError> {
        let conv = conversation_id.to_string();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "DELETE FROM message_history WHERE conversation_id = ?1",
                    rusqlite::params![conv],
                )?;
                conn.execute(
                    "DELETE FROM reactions WHERE conversation_id = ?1",
                    rusqlite::params![conv],
                )?;
                conn.execute(
                    "DELETE FROM message_revisions WHERE conversation_id = ?1",
                    rusqlite::params![conv],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Load the prior bodies of an edited message, oldest first, for the
    /// edit-history sheet.
    pub async fn load_revisions(
        &self,
        conversation_id: &str,
        author: &str,
        target_sent_at: Timestamp,
    ) -> Result<Vec<MessageRevision>, StoreError> {
        let conv = conversation_id.to_string();
        let author = author.to_string();
        let sent = target_sent_at.as_millis();
        self.conn
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT body, replaced_at FROM message_revisions
                     WHERE conversation_id = ?1 AND author_did = ?2 AND target_sent_at = ?3
                     ORDER BY replaced_at ASC",
                )?;
                let rows = stmt.query_map(rusqlite::params![conv, author, sent], |row| {
                    Ok(MessageRevision {
                        body: row.get(0)?,
                        replaced_at: Timestamp(row.get(1)?),
                    })
                })?;
                rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)
    }

    // ── Reactions (docs/33-reactions.md) ──────────────────────────────────

    /// Upsert a reactor's reaction on a target message. The PK enforces one
    /// reaction per (target, reactor), so this replaces any prior emoji from
    /// the same reactor (Signal's one-per-person rule).
    pub async fn upsert_reaction(&self, r: &ReactionRow) -> Result<(), StoreError> {
        let conv = r.conversation_id.clone();
        let author = r.target_author.clone();
        let sent = r.target_sent_at.as_millis();
        let reactor = r.reactor_did.clone();
        let emoji = r.emoji.clone();
        let reacted = r.reacted_at.as_millis();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO reactions
                     (conversation_id, target_author, target_sent_at, reactor_did, emoji, reacted_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![conv, author, sent, reactor, emoji, reacted],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Remove a reactor's reaction from a target message (tapping their own
    /// emoji again). No-op if none exists.
    pub async fn remove_reaction(
        &self,
        conversation_id: &str,
        target_author: &str,
        target_sent_at: Timestamp,
        reactor_did: &str,
    ) -> Result<(), StoreError> {
        let conv = conversation_id.to_string();
        let author = target_author.to_string();
        let sent = target_sent_at.as_millis();
        let reactor = reactor_did.to_string();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "DELETE FROM reactions
                     WHERE conversation_id = ?1 AND target_author = ?2 AND target_sent_at = ?3 AND reactor_did = ?4",
                    rusqlite::params![conv, author, sent, reactor],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Load all reactions in a conversation. The UI clusters them by target
    /// `(target_author, target_sent_at)`.
    pub async fn load_reactions(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<ReactionRow>, StoreError> {
        let conv = conversation_id.to_string();
        self.conn
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT conversation_id, target_author, target_sent_at, reactor_did, emoji, reacted_at
                     FROM reactions
                     WHERE conversation_id = ?1
                     ORDER BY reacted_at ASC",
                )?;
                let rows = stmt.query_map([&conv], |row| {
                    Ok(ReactionRow {
                        conversation_id: row.get(0)?,
                        target_author: row.get(1)?,
                        target_sent_at: Timestamp(row.get(2)?),
                        reactor_did: row.get(3)?,
                        emoji: row.get(4)?,
                        reacted_at: Timestamp(row.get(5)?),
                    })
                })?;
                rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
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
    /// Number of times this message has been edited (drives the human edit cap).
    pub edit_count: u32,
    /// Some = FOR_EVERYONE tombstone (body cleared, reactions dropped),
    /// carrying the unix-millis deletion time. None = live message.
    pub deleted_at: Option<Timestamp>,
    /// 0 = normal chat message; >0 = a system/metadata timeline entry
    /// (docs/03 §3.6 group events). Renderers show kind>0 rows as a centered
    /// grey line rather than a chat bubble.
    pub kind: i64,
    /// JSON blob for system rows (event kind + actor/target DIDs), letting the
    /// UI re-render localized text from structured data after a restart. NULL
    /// for normal chat messages.
    pub metadata: Option<String>,
    /// Disappearing-messages timer for this message in seconds (docs/03 §5);
    /// `0` = no expiry. Stamped from the conversation timer at send time.
    pub expire_timer_secs: i64,
    /// Unix-millis deletion deadline once the countdown has started (on read),
    /// or `None` if not started / no timer. The row is hard-deleted past this.
    pub expire_at: Option<Timestamp>,
}

/// A reaction on a message, keyed by the target's wire identity.
#[derive(Debug, Clone)]
pub struct ReactionRow {
    pub conversation_id: String,
    pub target_author: String,
    pub target_sent_at: Timestamp,
    pub reactor_did: String,
    pub emoji: String,
    pub reacted_at: Timestamp,
}

/// A prior body of an edited message, for the edit-history sheet.
#[derive(Debug, Clone)]
pub struct MessageRevision {
    pub body: String,
    pub replaced_at: Timestamp,
}
