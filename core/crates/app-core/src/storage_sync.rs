//! Storage-service sync engine (docs/05-device-data-sync.md §3, §4, §6).
//!
//! The only client component that talks to the storage service. Feature code
//! just mutates its domain table; a trigger marks the `storage_sync` sidecar
//! dirty (store crate), and [`sync`] reconciles: it pulls everything newer than
//! the cursor and writes it through to the domain tables, then pushes every
//! dirty row (built on demand from its domain table) with per-record CAS.
//!
//! ## Record framing (§4)
//!
//! - `record_id = HMAC-SHA256(storage_key, u16_be(TYPE_TAG) ‖ logical_key)[..16]`
//!   — deterministic so two devices address the same record without a shared
//!   manifest; opaque so the server learns neither type nor key.
//! - Ciphertext envelope: `version(1) ‖ nonce(12) ‖ AES-256-GCM(storage_key, plaintext)`
//!   where `plaintext = u16_be(TYPE_TAG) ‖ u16_be(key_len) ‖ logical_key ‖ payload`.
//!
//! The TYPE_TAG and logical_key live **inside** the authenticated plaintext, not
//! in the clear and not as AAD. This is forced by two constraints that the doc's
//! "bind TYPE_TAG as AAD" phrasing can't both satisfy: (1) the server must stay
//! type-blind, so the tag can't be cleartext; (2) a pull only receives the
//! opaque `record_id`, which can't be reversed, so the tag/key needed to *route*
//! a pulled record must come out of decryption. GCM still binds the tag to the
//! content (it's authenticated plaintext), and the pull re-derives `record_id`
//! from the recovered (tag, key) and checks it matches what the server returned —
//! so a record still can't masquerade under the wrong type or id.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use async_trait::async_trait;
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::error::AppError;

type HmacSha256 = Hmac<Sha256>;

const RECORD_VERSION: u8 = 1;
const NONCE_LEN: usize = 12;
/// Matches the server delta-pull page size in docs/05 §6.
const PULL_LIMIT: i64 = 500;

// ── TYPE_TAGs ────────────────────────────────────────────────────────────────
// Append-only and NEVER reused: a reused tag would alias two types onto one
// record_id space (silent corruption). The full enum-reuse enforcement is
// doc 05's OPEN item; for now the registry panics on a duplicate tag.
pub const TYPE_GROUP_KEY: u16 = 1;

// ── Record crypto (§4) ───────────────────────────────────────────────────────

/// `record_id = HMAC-SHA256(storage_key, u16_be(tag) ‖ logical_key)[..16]`.
pub fn record_id(storage_key: &[u8; 32], type_tag: u16, logical_key: &str) -> Vec<u8> {
    let mut mac =
        <HmacSha256 as Mac>::new_from_slice(storage_key).expect("HMAC accepts any key length");
    mac.update(&type_tag.to_be_bytes());
    mac.update(logical_key.as_bytes());
    mac.finalize().into_bytes()[..16].to_vec()
}

/// Seal a record into the ciphertext envelope. `payload` is empty for a tombstone.
fn seal(
    storage_key: &[u8; 32],
    type_tag: u16,
    logical_key: &str,
    payload: &[u8],
) -> Result<Vec<u8>, AppError> {
    let key_bytes = logical_key.as_bytes();
    if key_bytes.len() > u16::MAX as usize {
        return Err(AppError::Protocol("storage logical_key too long".into()));
    }
    let mut plaintext = Vec::with_capacity(2 + 2 + key_bytes.len() + payload.len());
    plaintext.extend_from_slice(&type_tag.to_be_bytes());
    plaintext.extend_from_slice(&(key_bytes.len() as u16).to_be_bytes());
    plaintext.extend_from_slice(key_bytes);
    plaintext.extend_from_slice(payload);

    let cipher = Aes256Gcm::new(storage_key.into());
    let nonce_bytes: [u8; NONCE_LEN] = rand::Rng::random(&mut rand::rng());
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_slice())
        .map_err(|e| AppError::Protocol(format!("storage record encryption failed: {e}")))?;

    let mut out = Vec::with_capacity(1 + NONCE_LEN + ciphertext.len());
    out.push(RECORD_VERSION);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Open a sealed record, recovering `(type_tag, logical_key, payload)`.
fn open(storage_key: &[u8; 32], blob: &[u8]) -> Result<(u16, String, Vec<u8>), AppError> {
    if blob.len() < 1 + NONCE_LEN + 16 {
        return Err(AppError::Protocol("storage record too short".into()));
    }
    if blob[0] != RECORD_VERSION {
        return Err(AppError::Protocol(format!(
            "unsupported storage record version: {}",
            blob[0]
        )));
    }
    let (nonce_bytes, ciphertext) = blob[1..].split_at(NONCE_LEN);
    let nonce = Nonce::from_slice(nonce_bytes);
    let cipher = Aes256Gcm::new(storage_key.into());
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| AppError::Protocol("storage record decryption failed".into()))?;

    if plaintext.len() < 4 {
        return Err(AppError::Protocol("storage record plaintext too short".into()));
    }
    let type_tag = u16::from_be_bytes([plaintext[0], plaintext[1]]);
    let key_len = u16::from_be_bytes([plaintext[2], plaintext[3]]) as usize;
    if plaintext.len() < 4 + key_len {
        return Err(AppError::Protocol("storage record key length out of range".into()));
    }
    let logical_key = String::from_utf8(plaintext[4..4 + key_len].to_vec())
        .map_err(|_| AppError::Protocol("storage record logical_key not utf8".into()))?;
    let payload = plaintext[4 + key_len..].to_vec();
    Ok((type_tag, logical_key, payload))
}

// ── Adapters & registry (§3.2, §3.3) ─────────────────────────────────────────

/// One synced record type, viewed byte-oriented and object-safe. The engine
/// routes pulled records to `apply` and reads dirty rows via `read`. (Stage 2
/// implements this object-safe trait directly; the typed `SyncedType` bridge
/// that makes adding a type a one-liner is the stage-3 ergonomic payoff.)
#[async_trait]
pub trait SyncAdapter: Send + Sync {
    fn type_tag(&self) -> u16;

    /// Apply a pulled record to its domain table. `payload = None` is a tombstone.
    async fn apply(
        &self,
        store: &store::Store,
        logical_key: &str,
        payload: Option<&[u8]>,
    ) -> Result<(), AppError>;

    /// Read a domain row and serialize its payload for push. `None` if the row
    /// no longer exists (the engine then pushes a tombstone).
    async fn read(&self, store: &store::Store, logical_key: &str)
        -> Result<Option<Vec<u8>>, AppError>;
}

/// The set of adapters the client knows about, keyed by TYPE_TAG.
pub struct SyncRegistry {
    adapters: Vec<Box<dyn SyncAdapter>>,
}

impl SyncRegistry {
    pub fn new() -> Self {
        Self { adapters: Vec::new() }
    }

    /// Register an adapter. Panics on a duplicate TYPE_TAG (a programming error
    /// that would corrupt the record_id space).
    pub fn add(&mut self, adapter: Box<dyn SyncAdapter>) {
        let tag = adapter.type_tag();
        assert!(
            self.adapters.iter().all(|a| a.type_tag() != tag),
            "duplicate storage TYPE_TAG {tag}"
        );
        self.adapters.push(adapter);
    }

    pub fn get(&self, type_tag: u16) -> Option<&dyn SyncAdapter> {
        self.adapters
            .iter()
            .find(|a| a.type_tag() == type_tag)
            .map(|a| a.as_ref())
    }

    /// The default registry: every synced type the client currently knows.
    pub fn default_registry() -> Self {
        let mut reg = Self::new();
        reg.add(Box::new(GroupKeyAdapter));
        reg
    }
}

impl Default for SyncRegistry {
    fn default() -> Self {
        Self::default_registry()
    }
}

// ── Group-key adapter (the one stage-2 adapter) ──────────────────────────────

/// Syncs group master keys, which already live in the `groups` domain table.
/// `logical_key` is the group_id. The payload is `master_key(32) ‖ utf8(host)`
/// — `master_key` is a fixed 32 bytes so the split is unambiguous, and those
/// two fields are all the server can't otherwise supply (group state/policy are
/// re-fetched). Protobuf is the doc's recommended codec for multi-field records;
/// this fixed-prefix+tail layout needs none and is swappable per adapter.
pub struct GroupKeyAdapter;

fn encode_group(master_key: &[u8], hosting_server_url: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(32 + hosting_server_url.len());
    v.extend_from_slice(master_key);
    v.extend_from_slice(hosting_server_url.as_bytes());
    v
}

fn decode_group(bytes: &[u8]) -> Result<([u8; 32], String), AppError> {
    if bytes.len() < 32 {
        return Err(AppError::Protocol("group-key record too short".into()));
    }
    let mut mk = [0u8; 32];
    mk.copy_from_slice(&bytes[..32]);
    let url = String::from_utf8(bytes[32..].to_vec())
        .map_err(|_| AppError::Protocol("group-key hosting url not utf8".into()))?;
    Ok((mk, url))
}

#[async_trait]
impl SyncAdapter for GroupKeyAdapter {
    fn type_tag(&self) -> u16 {
        TYPE_GROUP_KEY
    }

    async fn apply(
        &self,
        store: &store::Store,
        logical_key: &str,
        payload: Option<&[u8]>,
    ) -> Result<(), AppError> {
        match payload {
            None => {
                store.delete_group(logical_key).await?;
                Ok(())
            }
            Some(bytes) => {
                let (master_key, host) = decode_group(bytes)?;
                // Reuse the inbound-group-context path: it derives group_id from
                // master_key and inserts a default-policy row if absent (the rest
                // of group state is fetched separately). Idempotent on re-pull.
                crate::groups::store_inbound_group_context(store, &master_key, &host).await?;
                Ok(())
            }
        }
    }

    async fn read(
        &self,
        store: &store::Store,
        logical_key: &str,
    ) -> Result<Option<Vec<u8>>, AppError> {
        match store.load_group(logical_key).await? {
            Some(g) => Ok(Some(encode_group(&g.master_key, &g.hosting_server_url))),
            None => Ok(None),
        }
    }
}

// ── The engine (§6) ──────────────────────────────────────────────────────────

/// Reconcile the local durable-state store with the authoritative server:
/// pull everything newer than the cursor (write-through to domain tables), then
/// push every dirty row with per-record CAS. A no-op if no storage key is
/// provisioned yet.
pub async fn sync(
    store: &store::Store,
    client: &net::Client,
    registry: &SyncRegistry,
) -> Result<(), AppError> {
    let storage_key = match store.load_storage_key().await? {
        Some(k) => k,
        None => return Ok(()),
    };

    pull(store, client, registry, &storage_key).await?;
    push(store, client, registry, &storage_key).await?;
    Ok(())
}

async fn pull(
    store: &store::Store,
    client: &net::Client,
    registry: &SyncRegistry,
    storage_key: &[u8; 32],
) -> Result<(), AppError> {
    let mut cursor = store.storage_cursor().await?;
    loop {
        let page = client.storage_pull(cursor, PULL_LIMIT).await?;
        for it in &page.items {
            // Recover (tag, logical_key, payload) from the sealed blob. Even a
            // tombstone carries a sealed header (empty payload), so it routes.
            let (tag, logical_key, payload) = match open(storage_key, &it.ciphertext) {
                Ok(parts) => parts,
                Err(e) => {
                    tracing::warn!("[storage] skipping undecryptable record: {e}");
                    continue;
                }
            };

            // Integrity: the server-returned record_id must equal the one
            // derived from the recovered (tag, key) — else the ciphertext was
            // served under the wrong id.
            if record_id(storage_key, tag, &logical_key) != it.record_id {
                tracing::warn!("[storage] record_id mismatch for tag {tag}; skipping");
                continue;
            }

            // Last-writer-wins: skip anything we already have at >= this version.
            if it.version <= store.sync_version(tag, &logical_key).await? {
                continue;
            }

            match registry.get(tag) {
                Some(adapter) => {
                    let applied = if it.deleted {
                        adapter.apply(store, &logical_key, None).await
                    } else {
                        adapter.apply(store, &logical_key, Some(&payload)).await
                    };
                    if let Err(e) = applied {
                        tracing::warn!("[storage] apply failed for tag {tag}: {e}");
                        continue;
                    }
                    // Record the new server version and clear dirty (the apply's
                    // own domain write re-fired the trigger; this clears it).
                    store
                        .set_sync_meta(tag, &logical_key, it.version, false, it.deleted)
                        .await?;
                }
                None => {
                    // A record type this client build doesn't know yet. Record
                    // the version so we don't reprocess it every pull, but leave
                    // the payload unapplied for a future build.
                    tracing::warn!("[storage] no adapter for tag {tag}; recording version only");
                    store
                        .set_sync_meta(tag, &logical_key, it.version, false, it.deleted)
                        .await?;
                }
            }
        }
        cursor = page.next_cursor;
        if !page.has_more {
            break;
        }
    }
    store.set_storage_cursor(cursor).await?;
    Ok(())
}

async fn push(
    store: &store::Store,
    client: &net::Client,
    registry: &SyncRegistry,
    storage_key: &[u8; 32],
) -> Result<(), AppError> {
    let dirty = store.dirty_records().await?;
    if dirty.is_empty() {
        return Ok(());
    }

    let mut writes = Vec::with_capacity(dirty.len());
    // record_id → (tag, logical_key), to map applied/conflict results back.
    let mut by_record: Vec<(Vec<u8>, u16, String)> = Vec::with_capacity(dirty.len());

    for d in &dirty {
        let rid = record_id(storage_key, d.type_tag, &d.logical_key);

        // Decide payload: a tombstone, or the live row read via its adapter. If
        // the row vanished out from under a non-deleted dirty mark, push a
        // tombstone instead.
        let (deleted, payload) = if d.deleted {
            (true, Vec::new())
        } else {
            match registry.get(d.type_tag) {
                Some(adapter) => match adapter.read(store, &d.logical_key).await? {
                    Some(p) => (false, p),
                    None => (true, Vec::new()),
                },
                None => {
                    tracing::warn!(
                        "[storage] dirty row with unknown tag {}; skipping push",
                        d.type_tag
                    );
                    continue;
                }
            }
        };

        let ciphertext = seal(storage_key, d.type_tag, &d.logical_key, &payload)?;
        writes.push(net::StorageWrite {
            record_id: rid.clone(),
            expected_version: d.version,
            deleted,
            ciphertext,
        });
        by_record.push((rid, d.type_tag, d.logical_key.clone()));
    }

    if writes.is_empty() {
        return Ok(());
    }

    let result = client.storage_push(&writes).await?;

    for a in &result.applied {
        if let Some((_, tag, key)) = by_record.iter().find(|(rid, _, _)| *rid == a.record_id) {
            store.set_sync_meta_clean(*tag, key, a.version).await?;
        }
    }
    // Conflicts are left dirty: the next pull re-merges the server's version and
    // a subsequent push retries against it (§5, §9).
    for c in &result.conflicts {
        tracing::debug!(
            "[storage] push conflict at version {}; will retry after re-pull",
            c.current_version
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_id_is_deterministic_and_scoped() {
        let key = [1u8; 32];
        let base = record_id(&key, TYPE_GROUP_KEY, "g1");
        assert_eq!(base.len(), 16);
        assert_eq!(base, record_id(&key, TYPE_GROUP_KEY, "g1"));
        // Different key, tag, or logical_key → different id.
        assert_ne!(base, record_id(&key, TYPE_GROUP_KEY, "g2"));
        assert_ne!(base, record_id(&key, TYPE_GROUP_KEY + 1, "g1"));
        assert_ne!(base, record_id(&[2u8; 32], TYPE_GROUP_KEY, "g1"));
    }

    #[test]
    fn seal_open_round_trips_tag_key_and_payload() {
        let key = [9u8; 32];
        let blob = seal(&key, TYPE_GROUP_KEY, "group-xyz", b"payload-bytes").unwrap();
        let (tag, lk, payload) = open(&key, &blob).unwrap();
        assert_eq!(tag, TYPE_GROUP_KEY);
        assert_eq!(lk, "group-xyz");
        assert_eq!(payload, b"payload-bytes");
        assert_eq!(blob[0], RECORD_VERSION);
    }

    #[test]
    fn tombstone_header_round_trips_with_empty_payload() {
        let key = [9u8; 32];
        let blob = seal(&key, TYPE_GROUP_KEY, "gone", b"").unwrap();
        let (tag, lk, payload) = open(&key, &blob).unwrap();
        assert_eq!(tag, TYPE_GROUP_KEY);
        assert_eq!(lk, "gone");
        assert!(payload.is_empty());
    }

    #[test]
    fn open_rejects_bad_version_tamper_and_wrong_key() {
        let key = [9u8; 32];
        let blob = seal(&key, TYPE_GROUP_KEY, "g", b"x").unwrap();

        let mut bad_version = blob.clone();
        bad_version[0] = 0xFF;
        assert!(open(&key, &bad_version).is_err());

        let mut tampered = blob.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 0x01;
        assert!(open(&key, &tampered).is_err(), "GCM must reject a flipped byte");

        assert!(open(&[8u8; 32], &blob).is_err(), "wrong key must fail");
    }

    #[test]
    fn group_payload_round_trips() {
        let enc = encode_group(&[5u8; 32], "https://hs.example");
        let (mk, url) = decode_group(&enc).unwrap();
        assert_eq!(mk, [5u8; 32]);
        assert_eq!(url, "https://hs.example");
    }

    #[test]
    #[should_panic(expected = "duplicate storage TYPE_TAG")]
    fn registry_rejects_duplicate_tag() {
        let mut reg = SyncRegistry::new();
        reg.add(Box::new(GroupKeyAdapter));
        reg.add(Box::new(GroupKeyAdapter)); // same tag → panic
    }
}
