//! Local persistence of message attachment pointers (docs/35-attachments.md).
//!
//! When a message with attachments is saved to history, its decrypted pointers
//! are persisted here so the UI can render metadata (and a thumbnail) without
//! re-fetching, and track per-blob download state. The full blob is never
//! stored in this table — once downloaded it lands on disk and `local_path`
//! records where.

use types::Timestamp;

use crate::{db::IdentityStore, error::StoreError};

/// A decrypted attachment pointer plus local download state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentRow {
    /// Local row UUID.
    pub id: String,
    /// `message_history.id` this attachment belongs to.
    pub message_id: String,
    /// Position within the message (album order).
    pub ordinal: i64,
    /// Download URL on the hosting homeserver.
    pub url: String,
    /// MIME type.
    pub content_type: String,
    /// 64-byte attachment key (`aes ‖ hmac`).
    pub enc_key: Vec<u8>,
    /// 32-byte SHA-256 of the stored ciphertext blob.
    pub digest: Vec<u8>,
    /// Unpadded plaintext size in bytes.
    pub size_bytes: i64,
    pub file_name: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub duration_ms: Option<i64>,
    pub blurhash: Option<String>,
    /// Small decrypted preview (downscaled JPEG) for instant render.
    pub thumbnail: Option<Vec<u8>>,
    pub caption: Option<String>,
    /// Bitset (VOICE_NOTE, GIF, ...).
    pub flags: i64,
    /// Filesystem path of the decrypted full blob once downloaded; `None`
    /// until then.
    pub local_path: Option<String>,
    /// When the full blob was downloaded; `None` until then.
    pub downloaded_at: Option<Timestamp>,
}

impl IdentityStore {
    /// Replace the attachment rows for a message with `attachments`.
    ///
    /// Idempotent: existing rows for `message_id` are cleared first, so
    /// re-saving a message (e.g. a status transition) doesn't duplicate
    /// attachments. Local download state (`local_path`/`downloaded_at`) is
    /// carried over from any prior row at the same `ordinal`, so re-saving an
    /// already-downloaded message doesn't lose the on-disk file reference.
    pub async fn save_attachments(
        &self,
        message_id: &str,
        attachments: &[AttachmentRow],
    ) -> Result<(), StoreError> {
        let message_id = message_id.to_string();
        let rows = attachments.to_vec();
        self.conn
            .call(move |conn| {
                let tx = conn.transaction()?;

                // Preserve prior download state keyed by ordinal.
                let mut prior: std::collections::HashMap<i64, (Option<String>, Option<i64>)> =
                    std::collections::HashMap::new();
                {
                    let mut stmt = tx.prepare(
                        "SELECT ordinal, local_path, downloaded_at
                         FROM message_attachments WHERE message_id = ?1",
                    )?;
                    let mapped = stmt.query_map([&message_id], |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, Option<String>>(1)?,
                            row.get::<_, Option<i64>>(2)?,
                        ))
                    })?;
                    for r in mapped {
                        let (ord, lp, dl) = r?;
                        prior.insert(ord, (lp, dl));
                    }
                }

                tx.execute(
                    "DELETE FROM message_attachments WHERE message_id = ?1",
                    [&message_id],
                )?;

                for a in &rows {
                    let (local_path, downloaded_at) = match prior.get(&a.ordinal) {
                        // Prefer an explicitly-set new path, else inherit prior.
                        Some((lp, dl)) if a.local_path.is_none() => (lp.clone(), *dl),
                        _ => (a.local_path.clone(), a.downloaded_at.map(|t| t.as_millis())),
                    };
                    tx.execute(
                        "INSERT INTO message_attachments
                         (id, message_id, ordinal, url, content_type, enc_key, digest,
                          size_bytes, file_name, width, height, duration_ms, blurhash,
                          thumbnail, caption, flags, local_path, downloaded_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
                        rusqlite::params![
                            a.id,
                            a.message_id,
                            a.ordinal,
                            a.url,
                            a.content_type,
                            a.enc_key,
                            a.digest,
                            a.size_bytes,
                            a.file_name,
                            a.width,
                            a.height,
                            a.duration_ms,
                            a.blurhash,
                            a.thumbnail,
                            a.caption,
                            a.flags,
                            local_path,
                            downloaded_at,
                        ],
                    )?;
                }
                tx.commit()?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Load the attachments for a single message, ordered by `ordinal`.
    pub async fn load_attachments(
        &self,
        message_id: &str,
    ) -> Result<Vec<AttachmentRow>, StoreError> {
        let message_id = message_id.to_string();
        self.conn
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, message_id, ordinal, url, content_type, enc_key, digest,
                            size_bytes, file_name, width, height, duration_ms, blurhash,
                            thumbnail, caption, flags, local_path, downloaded_at
                     FROM message_attachments
                     WHERE message_id = ?1
                     ORDER BY ordinal ASC",
                )?;
                let rows = stmt.query_map([&message_id], row_to_attachment)?;
                rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Load every attachment for messages in a conversation, ordered by
    /// `(message_id, ordinal)`. The caller groups by `message_id` to attach
    /// them to the corresponding `HistoryMessage`.
    pub async fn load_attachments_for_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<AttachmentRow>, StoreError> {
        let conversation_id = conversation_id.to_string();
        self.conn
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT a.id, a.message_id, a.ordinal, a.url, a.content_type, a.enc_key,
                            a.digest, a.size_bytes, a.file_name, a.width, a.height,
                            a.duration_ms, a.blurhash, a.thumbnail, a.caption, a.flags,
                            a.local_path, a.downloaded_at
                     FROM message_attachments a
                     JOIN message_history m ON m.id = a.message_id
                     WHERE m.conversation_id = ?1
                     ORDER BY a.message_id ASC, a.ordinal ASC",
                )?;
                let rows = stmt.query_map([&conversation_id], row_to_attachment)?;
                rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Record that an attachment's full blob has been downloaded to `local_path`.
    pub async fn set_attachment_downloaded(
        &self,
        attachment_id: &str,
        local_path: &str,
        now: Timestamp,
    ) -> Result<(), StoreError> {
        let attachment_id = attachment_id.to_string();
        let local_path = local_path.to_string();
        let now_ms = now.as_millis();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "UPDATE message_attachments
                     SET local_path = ?2, downloaded_at = ?3
                     WHERE id = ?1",
                    rusqlite::params![attachment_id, local_path, now_ms],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Delete all attachment rows for messages in a conversation. Called from
    /// `delete_conversation` since there is no FK cascade.
    pub async fn delete_attachments_for_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<(), StoreError> {
        let conversation_id = conversation_id.to_string();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "DELETE FROM message_attachments
                     WHERE message_id IN (
                         SELECT id FROM message_history WHERE conversation_id = ?1
                     )",
                    [&conversation_id],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }
}

fn row_to_attachment(row: &rusqlite::Row) -> rusqlite::Result<AttachmentRow> {
    Ok(AttachmentRow {
        id: row.get(0)?,
        message_id: row.get(1)?,
        ordinal: row.get(2)?,
        url: row.get(3)?,
        content_type: row.get(4)?,
        enc_key: row.get(5)?,
        digest: row.get(6)?,
        size_bytes: row.get(7)?,
        file_name: row.get(8)?,
        width: row.get(9)?,
        height: row.get(10)?,
        duration_ms: row.get(11)?,
        blurhash: row.get(12)?,
        thumbnail: row.get(13)?,
        caption: row.get(14)?,
        flags: row.get(15)?,
        local_path: row.get(16)?,
        downloaded_at: row.get::<_, Option<i64>>(17)?.map(Timestamp),
    })
}
