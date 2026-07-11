//! Client-side storage for action-bound groups.
//!
//! Three tables (defined in `schema.rs`):
//!
//! - `groups` — per-group state: master key, hosting server, the latest
//!   `revision`, the most recently decrypted `GroupState` plaintext (cached
//!   for fast UI rendering), the server's policy mirror, and this device's
//!   `group_push_pseudonym` for the group.
//! - `group_credentials` — daily-rotated `AuthCredentialDid` blobs the
//!   client uses to authenticate on presentation-auth endpoints. One row
//!   per `(server_url, did, redemption_time)`.
//! - `group_server_params` — cached zkgroup server public params per
//!   homeserver. Populated lazily on first use; refreshed when the server
//!   advertises a new `version`.
//!
//! All blob columns are raw bytes — base64 is a wire-layer concern only.

use rusqlite::OptionalExtension as _;
use types::Timestamp;

use crate::{
    db::{DeviceStore, IdentityStore},
    error::StoreError,
};

/// Stored per-group row. Mirrors the columns in the `groups` table.
#[derive(Debug, Clone)]
pub struct GroupRow {
    /// 32-byte routing id (URL-safe-no-pad base64).
    pub group_id: String,
    /// 32-byte zkgroup master key.
    pub master_key: Vec<u8>,
    pub hosting_server_url: String,
    pub revision: i64,
    /// Bytes of `proto::groups::GroupState`, or empty if not yet fetched.
    pub encrypted_state_plaintext: Vec<u8>,
    pub policy: PolicyRow,
    /// This device's per-group push pseudonym (registered with the relay),
    /// or None until the user joins.
    pub group_push_pseudonym: Option<Vec<u8>>,
    pub created_at: Timestamp,
}

/// Server policy mirror. Integer values mirror the wire enum in
/// `server::routes::groups`: roles `0 = Member, 1 = Admin`, join policy
/// `0 = Closed, 1 = RequestToJoin, 2 = OpenLink`.
#[derive(Debug, Clone)]
pub struct PolicyRow {
    pub invite_members_role: i16,
    pub remove_members_role: i16,
    pub modify_title_role: i16,
    pub modify_description_role: i16,
    pub modify_expiry_role: i16,
    pub join_policy: i16,
    pub invite_link_password: Option<Vec<u8>>,
    pub announcement_only: bool,
}

impl PolicyRow {
    /// Default policy for a freshly-created group: everything Admin,
    /// closed to joins, no announcement-only.
    pub fn default_admin_only() -> Self {
        Self {
            invite_members_role: 1,
            remove_members_role: 1,
            modify_title_role: 1,
            modify_description_role: 1,
            modify_expiry_role: 1,
            join_policy: 0,
            invite_link_password: None,
            announcement_only: false,
        }
    }
}

// Workaround for rusqlite — Option<Vec<u8>> with the value column is a bit
// awkward when an empty vec must round-trip distinctly from NULL.
impl PolicyRow {
    fn invite_link_password_or_empty(&self) -> Vec<u8> {
        self.invite_link_password.clone().unwrap_or_default()
    }
}

impl IdentityStore {
    /// Insert a new group row. Replaces any existing row with the same
    /// `group_id` — useful for re-joining a group after leaving.
    pub async fn save_group(&self, row: &GroupRow) -> Result<(), StoreError> {
        let row = row.clone();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO groups (\
                        group_id, master_key, hosting_server_url, revision, \
                        encrypted_state_plaintext, \
                        policy_invite_members_role, policy_remove_members_role, \
                        policy_modify_title_role, policy_modify_description_role, \
                        policy_modify_expiry_role, policy_join_policy, \
                        policy_invite_link_password, policy_announcement_only, \
                        group_push_pseudonym, created_at\
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                    rusqlite::params![
                        row.group_id,
                        row.master_key,
                        row.hosting_server_url,
                        row.revision,
                        row.encrypted_state_plaintext,
                        row.policy.invite_members_role as i64,
                        row.policy.remove_members_role as i64,
                        row.policy.modify_title_role as i64,
                        row.policy.modify_description_role as i64,
                        row.policy.modify_expiry_role as i64,
                        row.policy.join_policy as i64,
                        row.policy.invite_link_password,
                        row.policy.announcement_only as i64,
                        row.group_push_pseudonym,
                        row.created_at.as_millis(),
                    ],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    pub async fn load_group(&self, group_id: &str) -> Result<Option<GroupRow>, StoreError> {
        let group_id = group_id.to_string();
        self.conn
            .call(move |conn| {
                conn.query_row(
                    "SELECT group_id, master_key, hosting_server_url, revision, \
                            encrypted_state_plaintext, \
                            policy_invite_members_role, policy_remove_members_role, \
                            policy_modify_title_role, policy_modify_description_role, \
                            policy_modify_expiry_role, policy_join_policy, \
                            policy_invite_link_password, policy_announcement_only, \
                            group_push_pseudonym, created_at \
                     FROM groups WHERE group_id = ?1",
                    rusqlite::params![group_id],
                    row_to_group,
                )
                .optional()
                .map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)
    }

    pub async fn list_groups(&self) -> Result<Vec<GroupRow>, StoreError> {
        self.conn
            .call(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT group_id, master_key, hosting_server_url, revision, \
                            encrypted_state_plaintext, \
                            policy_invite_members_role, policy_remove_members_role, \
                            policy_modify_title_role, policy_modify_description_role, \
                            policy_modify_expiry_role, policy_join_policy, \
                            policy_invite_link_password, policy_announcement_only, \
                            group_push_pseudonym, created_at \
                     FROM groups ORDER BY created_at ASC",
                )?;
                let rows = stmt
                    .query_map([], row_to_group)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Update the cached state + policy + revision after a successful
    /// fetch or apply.
    pub async fn update_group_state(
        &self,
        group_id: &str,
        revision: i64,
        encrypted_state_plaintext: Vec<u8>,
        policy: PolicyRow,
    ) -> Result<(), StoreError> {
        let group_id = group_id.to_string();
        self.conn
            .call(move |conn| {
                // Skip a no-op rewrite entirely. `fetch_group_state` calls this
                // on every group / group-info open, usually with byte-identical
                // values (the server revision hasn't moved). It is NOT enough to
                // make the UPDATE match zero rows: SQLite fires the commit hook
                // for any committed write transaction — even a 0-row UPDATE — and
                // that hook pokes the storage-sync scheduler, forcing a redundant
                // pull (and, before the row trigger was tamed, a push) on every
                // open. A read-only SELECT doesn't commit, so we compare first and
                // only issue the UPDATE when something actually differs. A genuine
                // change still writes (a sync then is acceptable).
                // `policy_invite_link_password` is nullable → NULL-safe `IS NOT`.
                // Bind to named locals so the same `params` slice can be reused
                // by both the compare query and the UPDATE (avoids temporaries
                // in the array literal).
                let invite_members = policy.invite_members_role as i64;
                let remove_members = policy.remove_members_role as i64;
                let modify_title = policy.modify_title_role as i64;
                let modify_description = policy.modify_description_role as i64;
                let modify_expiry = policy.modify_expiry_role as i64;
                let join_policy = policy.join_policy as i64;
                let announcement_only = policy.announcement_only as i64;
                let params = rusqlite::params![
                    group_id,
                    revision,
                    encrypted_state_plaintext,
                    invite_members,
                    remove_members,
                    modify_title,
                    modify_description,
                    modify_expiry,
                    join_policy,
                    policy.invite_link_password,
                    announcement_only,
                ];
                let differs: bool = conn.query_row(
                    "SELECT EXISTS( \
                       SELECT 1 FROM groups WHERE group_id = ?1 AND ( \
                            revision <> ?2 \
                            OR encrypted_state_plaintext <> ?3 \
                            OR policy_invite_members_role <> ?4 \
                            OR policy_remove_members_role <> ?5 \
                            OR policy_modify_title_role <> ?6 \
                            OR policy_modify_description_role <> ?7 \
                            OR policy_modify_expiry_role <> ?8 \
                            OR policy_join_policy <> ?9 \
                            OR policy_invite_link_password IS NOT ?10 \
                            OR policy_announcement_only <> ?11 ) )",
                    params,
                    |row| row.get::<_, i64>(0),
                )? != 0;
                if !differs {
                    return Ok(());
                }
                conn.execute(
                    "UPDATE groups SET revision = ?2, encrypted_state_plaintext = ?3, \
                            policy_invite_members_role = ?4, policy_remove_members_role = ?5, \
                            policy_modify_title_role = ?6, policy_modify_description_role = ?7, \
                            policy_modify_expiry_role = ?8, policy_join_policy = ?9, \
                            policy_invite_link_password = ?10, policy_announcement_only = ?11 \
                     WHERE group_id = ?1",
                    params,
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    pub async fn set_group_push_pseudonym(
        &self,
        group_id: &str,
        pseudonym: &[u8],
    ) -> Result<(), StoreError> {
        let group_id = group_id.to_string();
        let pseudonym = pseudonym.to_vec();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "UPDATE groups SET group_push_pseudonym = ?2 WHERE group_id = ?1",
                    rusqlite::params![group_id, pseudonym],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    pub async fn delete_group(&self, group_id: &str) -> Result<(), StoreError> {
        let group_id = group_id.to_string();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "DELETE FROM groups WHERE group_id = ?1",
                    rusqlite::params![group_id],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

}

impl DeviceStore {
    // ── credentials ─────────────────────────────────────────────────────

    pub async fn save_group_credential(
        &self,
        server_url: &str,
        did: &str,
        redemption_time: u64,
        bytes: &[u8],
        sender_cert: &[u8],
        sender_cert_expires_at_unix_millis: u64,
    ) -> Result<(), StoreError> {
        let server_url = server_url.to_string();
        let did = did.to_string();
        let bytes = bytes.to_vec();
        let sender_cert = sender_cert.to_vec();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO group_credentials \
                       (server_url, did, redemption_time, bytes, sender_cert, sender_cert_expires_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![
                        server_url,
                        did,
                        redemption_time as i64,
                        bytes,
                        sender_cert,
                        sender_cert_expires_at_unix_millis as i64,
                    ],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Returns `(credential_bytes, sender_cert_bytes, sender_cert_expires_at_unix_millis)`.
    pub async fn load_group_credential(
        &self,
        server_url: &str,
        did: &str,
        redemption_time: u64,
    ) -> Result<Option<(Vec<u8>, Vec<u8>, u64)>, StoreError> {
        let server_url = server_url.to_string();
        let did = did.to_string();
        self.conn
            .call(move |conn| {
                conn.query_row(
                    "SELECT bytes, sender_cert, sender_cert_expires_at FROM group_credentials \
                       WHERE server_url = ?1 AND did = ?2 AND redemption_time = ?3",
                    rusqlite::params![server_url, did, redemption_time as i64],
                    |row| {
                        Ok((
                            row.get::<_, Vec<u8>>(0)?,
                            row.get::<_, Vec<u8>>(1)?,
                            row.get::<_, i64>(2)? as u64,
                        ))
                    },
                )
                .optional()
                .map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Drop credential rows older than `cutoff_redemption_time` (Unix
    /// seconds, day-aligned). Called opportunistically on credential save.
    pub async fn prune_group_credentials(
        &self,
        cutoff_redemption_time: u64,
    ) -> Result<(), StoreError> {
        self.conn
            .call(move |conn| {
                conn.execute(
                    "DELETE FROM group_credentials WHERE redemption_time < ?1",
                    rusqlite::params![cutoff_redemption_time as i64],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    // ── server params ───────────────────────────────────────────────────

    pub async fn save_group_server_params(
        &self,
        server_url: &str,
        version: i32,
        bytes: &[u8],
        sender_cert_trust_root: &[u8],
    ) -> Result<(), StoreError> {
        let server_url = server_url.to_string();
        let bytes = bytes.to_vec();
        let trust_root = sender_cert_trust_root.to_vec();
        let fetched_at = Timestamp::now().as_millis();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO group_server_params \
                       (server_url, version, bytes, sender_cert_trust_root, fetched_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![server_url, version, bytes, trust_root, fetched_at],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    pub async fn load_group_server_params(
        &self,
        server_url: &str,
    ) -> Result<Option<(i32, Vec<u8>, Vec<u8>)>, StoreError> {
        let server_url = server_url.to_string();
        self.conn
            .call(move |conn| {
                conn.query_row(
                    "SELECT version, bytes, sender_cert_trust_root FROM group_server_params \
                     WHERE server_url = ?1",
                    rusqlite::params![server_url],
                    |row| {
                        Ok((
                            row.get::<_, i32>(0)?,
                            row.get::<_, Vec<u8>>(1)?,
                            row.get::<_, Vec<u8>>(2)?,
                        ))
                    },
                )
                .optional()
                .map_err(Into::into)
            })
            .await
            .map_err(StoreError::Db)
    }

    // ── pending (undecryptable) group ciphertext ────────────────────────
    //
    // A group message whose Sender Key isn't installed yet is buffered here
    // and retried when the sender's SKDM arrives. See the `pending_group_ciphertext`
    // schema note. The buffered `ciphertext` is always the inner SenderKeyMessage.

    /// Buffer one undecryptable group ciphertext for later retry.
    pub async fn buffer_pending_group_ciphertext(
        &self,
        group_id: &str,
        sender_did: &str,
        sender_device_id: u32,
        ciphertext: &[u8],
        server_id: Option<i64>,
    ) -> Result<(), StoreError> {
        let group_id = group_id.to_string();
        let sender_did = sender_did.to_string();
        let ciphertext = ciphertext.to_vec();
        let received_at_ms = Timestamp::now().as_millis();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO pending_group_ciphertext \
                       (group_id, sender_did, sender_device_id, ciphertext, server_id, received_at_ms) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![
                        group_id,
                        sender_did,
                        sender_device_id as i64,
                        ciphertext,
                        server_id,
                        received_at_ms,
                    ],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Load every buffered ciphertext from `(sender_did, sender_device_id)`,
    /// oldest first (retry must preserve receive order so the Sender Key chain
    /// advances correctly).
    pub async fn load_pending_group_ciphertext_for_sender(
        &self,
        sender_did: &str,
        sender_device_id: u32,
    ) -> Result<Vec<PendingGroupCiphertext>, StoreError> {
        let sender_did = sender_did.to_string();
        self.conn
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, group_id, sender_did, sender_device_id, ciphertext, \
                            server_id, received_at_ms \
                     FROM pending_group_ciphertext \
                     WHERE sender_did = ?1 AND sender_device_id = ?2 \
                     ORDER BY id ASC",
                )?;
                let rows = stmt
                    .query_map(
                        rusqlite::params![sender_did, sender_device_id as i64],
                        row_to_pending_group_ciphertext,
                    )?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Delete one buffered row by `id` (after a successful retry).
    pub async fn delete_pending_group_ciphertext(&self, id: i64) -> Result<(), StoreError> {
        self.conn
            .call(move |conn| {
                conn.execute(
                    "DELETE FROM pending_group_ciphertext WHERE id = ?1",
                    rusqlite::params![id],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }

    /// Drop buffered rows older than `cutoff_ms` (absolute epoch millis).
    /// Bounds the table when a sender's SKDM never arrives. Called
    /// opportunistically on buffer insert.
    pub async fn prune_pending_group_ciphertext(&self, cutoff_ms: i64) -> Result<(), StoreError> {
        self.conn
            .call(move |conn| {
                conn.execute(
                    "DELETE FROM pending_group_ciphertext WHERE received_at_ms < ?1",
                    rusqlite::params![cutoff_ms],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::Db)
    }
}

/// One buffered undecryptable group ciphertext awaiting its Sender Key.
#[derive(Debug, Clone)]
pub struct PendingGroupCiphertext {
    pub id: i64,
    pub group_id: String,
    pub sender_did: String,
    pub sender_device_id: u32,
    /// Inner SenderKeyMessage bytes — the input to `decrypt_group_content`.
    pub ciphertext: Vec<u8>,
    /// Original server message id, carried onto the recovered message for dedup.
    pub server_id: Option<i64>,
    pub received_at_ms: i64,
}

fn row_to_pending_group_ciphertext(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<PendingGroupCiphertext> {
    Ok(PendingGroupCiphertext {
        id: row.get::<_, i64>(0)?,
        group_id: row.get::<_, String>(1)?,
        sender_did: row.get::<_, String>(2)?,
        sender_device_id: row.get::<_, i64>(3)? as u32,
        ciphertext: row.get::<_, Vec<u8>>(4)?,
        server_id: row.get::<_, Option<i64>>(5)?,
        received_at_ms: row.get::<_, i64>(6)?,
    })
}

fn row_to_group(row: &rusqlite::Row<'_>) -> rusqlite::Result<GroupRow> {
    Ok(GroupRow {
        group_id: row.get::<_, String>(0)?,
        master_key: row.get::<_, Vec<u8>>(1)?,
        hosting_server_url: row.get::<_, String>(2)?,
        revision: row.get::<_, i64>(3)?,
        encrypted_state_plaintext: row.get::<_, Vec<u8>>(4)?,
        policy: PolicyRow {
            invite_members_role: row.get::<_, i64>(5)? as i16,
            remove_members_role: row.get::<_, i64>(6)? as i16,
            modify_title_role: row.get::<_, i64>(7)? as i16,
            modify_description_role: row.get::<_, i64>(8)? as i16,
            modify_expiry_role: row.get::<_, i64>(9)? as i16,
            join_policy: row.get::<_, i64>(10)? as i16,
            invite_link_password: row.get::<_, Option<Vec<u8>>>(11)?,
            announcement_only: row.get::<_, i64>(12)? != 0,
        },
        group_push_pseudonym: row.get::<_, Option<Vec<u8>>>(13)?,
        created_at: Timestamp(row.get::<_, i64>(14)?),
    })
}

// Suppress "field never read" while consumers come online.
#[allow(dead_code)]
fn _used(policy: &PolicyRow) -> Vec<u8> {
    policy.invite_link_password_or_empty()
}
