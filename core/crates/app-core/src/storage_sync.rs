//! Storage-service sync engine (docs/05-device-data-sync.md §3, §4, §6).
//!
//! Users might have multiple devices. Those devices all need to durably share
//! some data: contacts, group keys, preferences, muted channels, etc.
//! Each homeserver provides an encrypted storage service to its users;
//! this is the module that connects with that service and syncs the data.
//!
//! The `store` module is responsible for the local storage of the data.
//! You can easily mark any local table as synced and we'll sync it for you.
//!
//! There are limits (about 8MB total, 8KB per record) so don't go overboard.
//!
//! This is the only client component that talks to the storage service. Feature code
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
use store::storage_sync::SyncTriggerSpec;
use types::Timestamp;

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
pub const TYPE_CONTACT: u16 = 2;
pub const TYPE_CONV_SETTINGS: u16 = 3;
pub const TYPE_CONTACT_PROFILE: u16 = 4;

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
/// routes pulled records to `apply` and reads dirty rows via `read`. Adapters
/// are not written by hand — they fall out of the typed [`SyncedType`] trait
/// via the SyncedType impl below.
#[async_trait]
pub trait SyncAdapter: Send + Sync {
    fn type_tag(&self) -> u16;

    /// The dirty-tracking triggers this type needs (§3.4). Collected by the
    /// registry and installed at account open.
    fn trigger_spec(&self) -> SyncTriggerSpec;

    /// Apply a pulled record to its domain table. `payload = None` is a tombstone.
    async fn apply(
        &self,
        store: &store::IdentityStore,
        logical_key: &str,
        payload: Option<&[u8]>,
    ) -> Result<(), AppError>;

    /// Read a domain row and serialize its payload for push. `None` if the row
    /// no longer exists (the engine then pushes a tombstone).
    async fn read(&self, store: &store::IdentityStore, logical_key: &str)
        -> Result<Option<Vec<u8>>, AppError>;
}

/// The typed, ergonomic view of a synced type (docs/05 §3.2) — the primary
/// goal of the whole design (§2.2): adding a synced type is an `impl` of this
/// trait plus one `reg.add(...)` line; encryption, CAS, the cursor, conflict
/// resolution, the push nudge, and recovery all then work unchanged.
///
/// `encode`/`decode` handle only the **payload** — the logical key is framed
/// separately by the engine into the sealed header (§4), so it is passed back
/// to `decode` rather than duplicated in the bytes. The store-touching methods
/// stay `async` (our store is async; the doc's sync signatures don't survive).
#[async_trait]
pub trait SyncedType: Send + Sync + 'static {
    /// Stable, globally-unique, NEVER-reused tag. Part of the record_id (§4).
    const TYPE_TAG: u16;
    /// Domain table this type lives in — for trigger generation (§3.4).
    const TABLE: &'static str;
    /// The table's natural-key column (e.g. `group_id`, `did`).
    const KEY_COLUMN: &'static str;

    type Record: Send;

    /// Serialize the payload (everything except the logical key).
    fn encode(record: &Self::Record) -> Vec<u8>;
    /// Rebuild a record from its logical key + payload.
    fn decode(logical_key: &str, bytes: &[u8]) -> Result<Self::Record, AppError>;

    /// Write a pulled record through into the domain table.
    async fn upsert(store: &store::IdentityStore, record: &Self::Record) -> Result<(), AppError>;
    /// Apply a tombstone: remove the domain row.
    async fn delete(store: &store::IdentityStore, logical_key: &str) -> Result<(), AppError>;
    /// Read the domain row for push. `None` ⇒ row gone ⇒ engine pushes a tombstone.
    async fn load(
        store: &store::IdentityStore,
        logical_key: &str,
    ) -> Result<Option<Self::Record>, AppError>;
}

/// Every [`SyncedType`] is a [`SyncAdapter`]. Authors only
/// ever write the typed trait; the registry only ever stores `dyn SyncAdapter`.
#[async_trait]
impl<T: SyncedType> SyncAdapter for T {
    fn type_tag(&self) -> u16 {
        T::TYPE_TAG
    }

    fn trigger_spec(&self) -> SyncTriggerSpec {
        SyncTriggerSpec::new(T::TABLE, T::KEY_COLUMN, T::TYPE_TAG)
    }

    async fn apply(
        &self,
        store: &store::IdentityStore,
        logical_key: &str,
        payload: Option<&[u8]>,
    ) -> Result<(), AppError> {
        match payload {
            None => T::delete(store, logical_key).await,
            Some(bytes) => {
                let record = T::decode(logical_key, bytes)?;
                T::upsert(store, &record).await
            }
        }
    }

    async fn read(
        &self,
        store: &store::IdentityStore,
        logical_key: &str,
    ) -> Result<Option<Vec<u8>>, AppError> {
        Ok(T::load(store, logical_key).await?.map(|r| T::encode(&r)))
    }
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

    /// The dirty-tracking trigger specs for every registered type (§3.4),
    /// installed via [`store::Store::install_sync_triggers`] at account open.
    pub fn trigger_specs(&self) -> Vec<SyncTriggerSpec> {
        self.adapters.iter().map(|a| a.trigger_spec()).collect()
    }

    /// The default registry: every synced type the client currently knows.
    pub fn default_registry() -> Self {
        let mut reg = Self::new();
        reg.add(Box::new(GroupKeyAdapter));
        reg.add(Box::new(ContactAdapter));
        reg.add(Box::new(ConvSettingsAdapter));
        reg.add(Box::new(ContactProfileAdapter));
        reg
    }
}

/// Install the dirty-tracking triggers for the default registry — but only when
/// storage sync is enabled for this account (a storage key is present). A bot
/// (no key, §11/opt-out) accrues no sidecar rows. Idempotent; safe every open.
pub async fn ensure_triggers(store: &store::IdentityStore) -> Result<(), AppError> {
    if store.load_storage_key().await?.is_none() {
        return Ok(());
    }
    store
        .install_sync_triggers(&SyncRegistry::default_registry().trigger_specs())
        .await?;
    Ok(())
}

impl Default for SyncRegistry {
    fn default() -> Self {
        Self::default_registry()
    }
}

// ── Group-key adapter (tag 1) ─────────────────────────────────────────────────

/// Syncs group master keys, which already live in the `groups` domain table.
/// `logical_key` is the group_id. The payload is `master_key(32) ‖ utf8(host)`
/// — `master_key` is a fixed 32 bytes so the split is unambiguous, and those
/// two fields are all the server can't otherwise supply (group state/policy are
/// re-fetched). Protobuf is the doc's recommended codec for multi-field records;
/// this fixed-prefix+tail layout needs none and is swappable per adapter.
pub struct GroupKeyAdapter;

pub struct GroupKeyRecord {
    pub master_key: [u8; 32],
    pub hosting_server_url: String,
}

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
impl SyncedType for GroupKeyAdapter {
    const TYPE_TAG: u16 = TYPE_GROUP_KEY;
    const TABLE: &'static str = "groups";
    const KEY_COLUMN: &'static str = "group_id";
    type Record = GroupKeyRecord;

    fn encode(record: &GroupKeyRecord) -> Vec<u8> {
        encode_group(&record.master_key, &record.hosting_server_url)
    }

    fn decode(_logical_key: &str, bytes: &[u8]) -> Result<GroupKeyRecord, AppError> {
        let (master_key, hosting_server_url) = decode_group(bytes)?;
        Ok(GroupKeyRecord {
            master_key,
            hosting_server_url,
        })
    }

    async fn upsert(store: &store::IdentityStore, record: &GroupKeyRecord) -> Result<(), AppError> {
        // Reuse the inbound-group-context path: it derives group_id from
        // master_key and inserts a default-policy row if absent (the rest of
        // group state is fetched separately). Idempotent on re-pull. The
        // group_id logical key is not needed — it falls out of master_key.
        crate::groups::store_inbound_group_context(
            store,
            &record.master_key,
            &record.hosting_server_url,
        )
        .await?;
        Ok(())
    }

    async fn delete(store: &store::IdentityStore, logical_key: &str) -> Result<(), AppError> {
        store.delete_group(logical_key).await?;
        Ok(())
    }

    async fn load(
        store: &store::IdentityStore,
        logical_key: &str,
    ) -> Result<Option<GroupKeyRecord>, AppError> {
        Ok(store.load_group(logical_key).await?.map(|g| GroupKeyRecord {
            master_key: {
                let mut mk = [0u8; 32];
                // group master keys are always 32 bytes; truncate/pad defensively.
                let n = g.master_key.len().min(32);
                mk[..n].copy_from_slice(&g.master_key[..n]);
                mk
            },
            hosting_server_url: g.hosting_server_url,
        }))
    }
}

// ── Contact adapter (tag 2) ───────────────────────────────────────────────────

/// Syncs the curated/blocked contacts list (`contacts` table). `logical_key` is
/// the DID. Payload is `is_curated(1) ‖ last_interaction_at(i64 BE 8) ‖
/// is_blocked(1)`. The trailing `is_blocked` byte is optional on decode: a
/// 9-byte payload written before the block list shipped reads as `false`, so
/// the format is backward- and forward-compatible. `has_pending_request` is
/// deliberately not carried — it's local inbox state, not durable identity
/// state. The contact's name/profile_key roam separately as
/// [`ContactProfileAdapter`] records.
pub struct ContactAdapter;

pub struct ContactRecord {
    pub did: String,
    pub is_curated: bool,
    pub last_interaction_at: i64,
    pub is_blocked: bool,
}

#[async_trait]
impl SyncedType for ContactAdapter {
    const TYPE_TAG: u16 = TYPE_CONTACT;
    const TABLE: &'static str = "contacts";
    const KEY_COLUMN: &'static str = "did";
    type Record = ContactRecord;

    fn encode(record: &ContactRecord) -> Vec<u8> {
        let mut v = Vec::with_capacity(10);
        v.push(record.is_curated as u8);
        v.extend_from_slice(&record.last_interaction_at.to_be_bytes());
        v.push(record.is_blocked as u8);
        v
    }

    fn decode(logical_key: &str, bytes: &[u8]) -> Result<ContactRecord, AppError> {
        if bytes.len() < 9 {
            return Err(AppError::Protocol("contact record too short".into()));
        }
        let is_curated = bytes[0] != 0;
        let last_interaction_at = i64::from_be_bytes(bytes[1..9].try_into().unwrap());
        // Trailing block byte is optional: legacy 9-byte records read as false.
        let is_blocked = bytes.get(9).is_some_and(|b| *b != 0);
        Ok(ContactRecord {
            did: logical_key.to_string(),
            is_curated,
            last_interaction_at,
            is_blocked,
        })
    }

    async fn upsert(store: &store::IdentityStore, record: &ContactRecord) -> Result<(), AppError> {
        // The engine only applies strictly-newer versions (LWW), so the pulled
        // record is authoritative: is_curated/recency take a monotonic MAX
        // (they never rewind), is_blocked is overwritten so an unblock on
        // another device propagates. See store::apply_synced_contact.
        store
            .apply_synced_contact(
                &record.did,
                record.is_curated,
                record.is_blocked,
                Timestamp(record.last_interaction_at),
            )
            .await?;
        Ok(())
    }

    async fn delete(store: &store::IdentityStore, logical_key: &str) -> Result<(), AppError> {
        store.delete_contact(logical_key).await?;
        Ok(())
    }

    async fn load(
        store: &store::IdentityStore,
        logical_key: &str,
    ) -> Result<Option<ContactRecord>, AppError> {
        Ok(store.load_contact(logical_key).await?.map(|c| ContactRecord {
            did: c.did,
            is_curated: c.is_curated,
            last_interaction_at: c.last_interaction_at.as_millis(),
            is_blocked: c.is_blocked,
        }))
    }
}

// ── Conversation-settings adapter (tag 3) ─────────────────────────────────────

/// Syncs per-conversation expiry timers (`conversation_settings` table).
/// `logical_key` is the conversation_id. Payload is the timer as 4-byte BE when
/// set, empty when there is no timer (distinct from a tombstone, which clears
/// the whole row via the `deleted` envelope flag).
pub struct ConvSettingsAdapter;

pub struct ConvSettingsRecord {
    pub conversation_id: String,
    pub expiry_secs: Option<u32>,
}

#[async_trait]
impl SyncedType for ConvSettingsAdapter {
    const TYPE_TAG: u16 = TYPE_CONV_SETTINGS;
    const TABLE: &'static str = "conversation_settings";
    const KEY_COLUMN: &'static str = "conversation_id";
    type Record = ConvSettingsRecord;

    fn encode(record: &ConvSettingsRecord) -> Vec<u8> {
        match record.expiry_secs {
            Some(v) => v.to_be_bytes().to_vec(),
            None => Vec::new(),
        }
    }

    fn decode(logical_key: &str, bytes: &[u8]) -> Result<ConvSettingsRecord, AppError> {
        let expiry_secs = match bytes.len() {
            0 => None,
            4 => Some(u32::from_be_bytes(bytes.try_into().unwrap())),
            _ => return Err(AppError::Protocol("conv-settings record malformed".into())),
        };
        Ok(ConvSettingsRecord {
            conversation_id: logical_key.to_string(),
            expiry_secs,
        })
    }

    async fn upsert(store: &store::IdentityStore, record: &ConvSettingsRecord) -> Result<(), AppError> {
        store
            .save_conversation_expiry(&record.conversation_id, record.expiry_secs)
            .await?;
        Ok(())
    }

    async fn delete(store: &store::IdentityStore, logical_key: &str) -> Result<(), AppError> {
        store.delete_conversation_settings(logical_key).await?;
        Ok(())
    }

    async fn load(
        store: &store::IdentityStore,
        logical_key: &str,
    ) -> Result<Option<ConvSettingsRecord>, AppError> {
        Ok(store
            .load_conversation_settings(logical_key)
            .await?
            .map(|r| ConvSettingsRecord {
                conversation_id: r.conversation_id,
                expiry_secs: r.expiry_secs,
            }))
    }
}

// ── Contact-profile adapter (tag 4) ───────────────────────────────────────────

/// Syncs cached contact profiles (`contact_profiles` table) so a fresh device
/// can render names and re-fetch updates. `logical_key` is the DID. Payload is
/// `fetched_at(i64 BE 8) ‖ key_len(u16 BE) ‖ profile_key ‖ utf8(display_name)`
/// — `profile_key` is length-prefixed since it is a variable-length blob.
pub struct ContactProfileAdapter;

#[async_trait]
impl SyncedType for ContactProfileAdapter {
    const TYPE_TAG: u16 = TYPE_CONTACT_PROFILE;
    const TABLE: &'static str = "contact_profiles";
    const KEY_COLUMN: &'static str = "did";
    type Record = store::profiles::ContactProfile;

    fn encode(record: &store::profiles::ContactProfile) -> Vec<u8> {
        let key = &record.profile_key;
        let name = record.display_name.as_bytes();
        let mut v = Vec::with_capacity(8 + 2 + key.len() + name.len());
        v.extend_from_slice(&record.fetched_at.as_millis().to_be_bytes());
        v.extend_from_slice(&(key.len() as u16).to_be_bytes());
        v.extend_from_slice(key);
        v.extend_from_slice(name);
        v
    }

    fn decode(
        logical_key: &str,
        bytes: &[u8],
    ) -> Result<store::profiles::ContactProfile, AppError> {
        if bytes.len() < 10 {
            return Err(AppError::Protocol("contact-profile record too short".into()));
        }
        let fetched_at = i64::from_be_bytes(bytes[0..8].try_into().unwrap());
        let key_len = u16::from_be_bytes(bytes[8..10].try_into().unwrap()) as usize;
        if bytes.len() < 10 + key_len {
            return Err(AppError::Protocol("contact-profile key_len out of range".into()));
        }
        let profile_key = bytes[10..10 + key_len].to_vec();
        let display_name = String::from_utf8(bytes[10 + key_len..].to_vec())
            .map_err(|_| AppError::Protocol("contact-profile name not utf8".into()))?;
        Ok(store::profiles::ContactProfile {
            did: logical_key.to_string(),
            display_name,
            profile_key,
            fetched_at: Timestamp(fetched_at),
        })
    }

    async fn upsert(
        store: &store::IdentityStore,
        record: &store::profiles::ContactProfile,
    ) -> Result<(), AppError> {
        store.upsert_contact_profile(record).await?;
        Ok(())
    }

    async fn delete(store: &store::IdentityStore, logical_key: &str) -> Result<(), AppError> {
        store.delete_contact_profile(logical_key).await?;
        Ok(())
    }

    async fn load(
        store: &store::IdentityStore,
        logical_key: &str,
    ) -> Result<Option<store::profiles::ContactProfile>, AppError> {
        Ok(store.load_contact_profile(logical_key).await?)
    }
}

// ── The engine (§6) ──────────────────────────────────────────────────────────

/// Debounce window: coalesce a burst of local writes into a single push.
const SCHEDULER_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(750);
/// Safety-net poll: catch a missed commit-hook poke or an offline-recovery
/// backlog even if nothing pokes us. The durable `dirty` bit outlives any
/// missed `Notify`, so this makes the scheduler self-healing (§6.1).
const SCHEDULER_SAFETY_POLL: std::time::Duration = std::time::Duration::from_secs(60);

/// The background push scheduler (docs/05 §6.1). Registers the store commit hook
/// (which pokes `AppCore::sync_notify` on every committed local write) and then
/// loops: on a poke or the safety-net tick, debounce and run [`sync`]. The
/// `Notify` coalesces a burst into one permit, and the loop is single-flight, so
/// a flurry of writes collapses to ~one sync.
///
/// Exits immediately for an account opted out of storage sync (no storage key —
/// e.g. a bot): no hook is registered and no work is scheduled. Self-exits when
/// the last `Arc<AppCore>` drops (the `Weak` upgrade fails).
pub(crate) async fn run_scheduler(weak: std::sync::Weak<crate::AppCore>) {
    // One-time setup: grab a store handle + the shared notify.
    let (store, notify) = {
        let Some(app) = weak.upgrade() else {
            return;
        };
        let store = app.inner.lock().await.store.clone();
        (store, app.sync_notify.clone())
    };

    // Opt-out gate: no storage key ⇒ nothing to schedule.
    match store.load_storage_key().await {
        Ok(Some(_)) => {}
        Ok(None) => return,
        Err(e) => {
            tracing::warn!("[storage] scheduler could not read storage key: {e}");
            return;
        }
    }

    // Wake the loop on every committed local write.
    let hook_notify = notify.clone();
    if let Err(e) = store
        .set_commit_hook(move || hook_notify.notify_one())
        .await
    {
        tracing::warn!("[storage] failed to register commit hook: {e}");
        return;
    }

    let mut poll = tokio::time::interval(SCHEDULER_SAFETY_POLL);
    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = notify.notified() => {}
            _ = poll.tick() => {}
        }
        // Coalesce a burst before syncing.
        tokio::time::sleep(SCHEDULER_DEBOUNCE).await;

        let Some(app) = weak.upgrade() else {
            break;
        };
        if let Err(e) = app.sync_storage_async().await {
            tracing::warn!("[storage] scheduled sync failed: {e}");
        }
    }
}

/// Reconcile the local durable-state store with the authoritative server:
/// pull everything newer than the cursor (write-through to domain tables), then
/// push every dirty row with per-record CAS. A no-op if no storage key is
/// provisioned yet.
/// Returns the number of remote records the pull applied to local domain
/// tables — callers use a non-zero count to know the conversation list may have
/// changed and emit a refresh signal.
pub async fn sync(
    store: &store::DeviceStore,
    client: &net::Client,
    registry: &SyncRegistry,
) -> Result<usize, AppError> {
    let storage_key = match store.load_storage_key().await? {
        Some(k) => k,
        None => return Ok(0),
    };

    let applied = pull(store, client, registry, &storage_key).await?;
    push(store, client, registry, &storage_key).await?;
    Ok(applied)
}

/// Pull remote records newer than the cursor, applying each to its domain
/// table. Returns how many were applied (skips for already-seen versions and
/// unknown tags don't count).
async fn pull(
    store: &store::DeviceStore,
    client: &net::Client,
    registry: &SyncRegistry,
    storage_key: &[u8; 32],
) -> Result<usize, AppError> {
    let mut applied_count = 0usize;
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
                    applied_count += 1;
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
    Ok(applied_count)
}

async fn push(
    store: &store::DeviceStore,
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

// ── Snapshot (docs/05 §7, §11) — PARKED, NOT IN USE ──────────────────────────
//
// `build_snapshot`/`restore_snapshot` (a whole-store backup blob for the passive
// non-authoritative accounts) are intentionally commented out: the underlying
// backup-placement design is being reconsidered and nothing calls them yet.
//
// The open concern (see docs/05-device-data-sync.md §7 "OPEN — backup placement
// under review"): the snapshot is a *separate kind of server storage* from the
// per-record `/items` store (different table, no `seq`/CAS, LWW on one blob), so
// a passive backup can NOT be transparently promoted to authoritative — doing so
// needs a snapshot-restore-then-full-item-reseed step. Before re-enabling this,
// take another pass at whether that two-storage-types + non-transparent-promotion
// model is right, vs. alternatives (backups as first-class item stores / one-way
// item replication / accepting per-record-LWW multi-master since records are
// already independent + LWW). Do NOT just uncomment — revisit the design first.
//
// The code below is preserved verbatim (it compiled + round-tripped in tests) so
// a future pass has the working serialize/restore-via-adapters core to build on.
/*
const SNAPSHOT_VERSION: u8 = 1;

/// Build a whole-store snapshot blob (docs/05 §7). Enumerates every synced
/// record from the sidecar, reads each live payload via its adapter (or a
/// tombstone), seals it, and frames the set. Returns the blob to `PUT` to a
/// backup account. A no-op empty-record snapshot is still a valid blob.
pub async fn build_snapshot(
    store: &store::DeviceStore,
    registry: &SyncRegistry,
    storage_key: &[u8; 32],
) -> Result<Vec<u8>, AppError> {
    let records = store.all_sync_records().await?;

    // Frame: [u8 version][u32 count] then per record
    //        [i64 version][u8 deleted][u32 ct_len][ct].
    let mut out = Vec::new();
    out.push(SNAPSHOT_VERSION);
    let mut count: u32 = 0;
    let mut body = Vec::new();
    for r in &records {
        let (deleted, payload) = if r.deleted {
            (true, Vec::new())
        } else {
            match registry.get(r.type_tag) {
                Some(adapter) => match adapter.read(store, &r.logical_key).await? {
                    Some(p) => (false, p),
                    None => (true, Vec::new()), // row vanished → tombstone
                },
                None => {
                    // Unknown type this build can't serialize; skip (the snapshot
                    // is best-effort whole-store, and a foreign record can't be
                    // read without its adapter).
                    tracing::warn!("[storage] snapshot skipping unknown tag {}", r.type_tag);
                    continue;
                }
            }
        };
        let ciphertext = seal(storage_key, r.type_tag, &r.logical_key, &payload)?;
        body.extend_from_slice(&r.version.to_be_bytes());
        body.push(deleted as u8);
        body.extend_from_slice(&(ciphertext.len() as u32).to_be_bytes());
        body.extend_from_slice(&ciphertext);
        count += 1;
    }
    out.extend_from_slice(&count.to_be_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

/// Restore a snapshot blob into the local store (docs/05 §11). Opens each sealed
/// record, applies it through its adapter (write-through to the domain table),
/// and records the sidecar version (clean, not dirty) — exactly like a pull.
/// Idempotent and LWW: a record already at ≥ its snapshot version is skipped.
/// Returns the number of records applied.
pub async fn restore_snapshot(
    store: &store::DeviceStore,
    registry: &SyncRegistry,
    storage_key: &[u8; 32],
    blob: &[u8],
) -> Result<usize, AppError> {
    let mut cur = blob;
    let take = |cur: &mut &[u8], n: usize| -> Result<Vec<u8>, AppError> {
        if cur.len() < n {
            return Err(AppError::Protocol("snapshot truncated".into()));
        }
        let (head, tail) = cur.split_at(n);
        *cur = tail;
        Ok(head.to_vec())
    };

    let version = take(&mut cur, 1)?[0];
    if version != SNAPSHOT_VERSION {
        return Err(AppError::Protocol(format!(
            "unsupported snapshot version: {version}"
        )));
    }
    let count = u32::from_be_bytes(take(&mut cur, 4)?.try_into().unwrap());

    let mut applied = 0usize;
    for _ in 0..count {
        let record_version = i64::from_be_bytes(take(&mut cur, 8)?.try_into().unwrap());
        let deleted = take(&mut cur, 1)?[0] != 0;
        let ct_len = u32::from_be_bytes(take(&mut cur, 4)?.try_into().unwrap()) as usize;
        let ciphertext = take(&mut cur, ct_len)?;

        let (tag, logical_key, payload) = open(storage_key, &ciphertext)?;

        // Integrity: the sealed (tag, key) must reproduce a consistent record_id
        // (matches the pull path's check; here it just guards a corrupt blob).
        let _ = record_id(storage_key, tag, &logical_key);

        // LWW: don't regress a record we already hold at an equal/newer version.
        if record_version <= store.sync_version(tag, &logical_key).await? && record_version != 0 {
            continue;
        }

        match registry.get(tag) {
            Some(adapter) => {
                let payload_opt = if deleted { None } else { Some(payload.as_slice()) };
                if let Err(e) = adapter.apply(store, &logical_key, payload_opt).await {
                    tracing::warn!("[storage] snapshot apply failed for tag {tag}: {e}");
                    continue;
                }
                store
                    .set_sync_meta(tag, &logical_key, record_version, false, deleted)
                    .await?;
                applied += 1;
            }
            None => {
                tracing::warn!("[storage] snapshot has unknown tag {tag}; skipping");
            }
        }
    }
    Ok(applied)
}
*/

#[cfg(test)]
mod tests {
    use super::*;

    // Snapshot tests are PARKED alongside `build_snapshot`/`restore_snapshot`
    // (see the "PARKED, NOT IN USE" banner above and docs/05 §7). Restore them
    // when the backup-placement design is revisited.
    /*
    #[tokio::test]
    async fn snapshot_build_then_restore_round_trips_records() {
        let key = [7u8; 32];
        let reg = SyncRegistry::default_registry();

        // Source store with two contacts (one curated, one not).
        let src = store::DeviceStore::open_in_memory().await.unwrap();
        src.save_storage_key(&key).await.unwrap();
        ensure_triggers(&src).await.unwrap();
        src.touch_contact("did:plc:bob", true, Timestamp(111))
            .await
            .unwrap();
        src.touch_contact("did:plc:carol", false, Timestamp(222))
            .await
            .unwrap();

        let blob = build_snapshot(&src, &reg, &key).await.unwrap();
        assert!(blob.len() > 5, "snapshot should carry framed records");

        // A fresh store hydrates entirely from the blob.
        let dst = store::DeviceStore::open_in_memory().await.unwrap();
        dst.save_storage_key(&key).await.unwrap();
        ensure_triggers(&dst).await.unwrap();
        let applied = restore_snapshot(&dst, &reg, &key, &blob).await.unwrap();
        assert_eq!(applied, 2);

        let bob = dst
            .load_contact("did:plc:bob")
            .await
            .unwrap()
            .expect("bob restored");
        assert!(bob.is_curated);
        assert_eq!(bob.last_interaction_at.as_millis(), 111);
        let carol = dst
            .load_contact("did:plc:carol")
            .await
            .unwrap()
            .expect("carol restored");
        assert!(!carol.is_curated);
        assert_eq!(carol.last_interaction_at.as_millis(), 222);
    }

    #[tokio::test]
    async fn snapshot_restore_rejects_unknown_version() {
        let key = [1u8; 32];
        let reg = SyncRegistry::default_registry();
        let dst = store::DeviceStore::open_in_memory().await.unwrap();
        // version byte 99 is not SNAPSHOT_VERSION.
        let bad = vec![99u8, 0, 0, 0, 0];
        assert!(restore_snapshot(&dst, &reg, &key, &bad).await.is_err());
    }
    */

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

    // ── Stage-3 adapters (SyncedType codecs) ──────────────────────────────────

    #[test]
    fn contact_payload_round_trips() {
        let r = ContactRecord {
            did: "did:plc:x".into(),
            is_curated: true,
            last_interaction_at: 1_700_000_000_000,
            is_blocked: true,
        };
        let bytes = ContactAdapter::encode(&r);
        assert_eq!(bytes.len(), 10);
        let back = ContactAdapter::decode("did:plc:x", &bytes).unwrap();
        assert_eq!(back.did, "did:plc:x"); // logical_key injected, not in payload
        assert!(back.is_curated);
        assert_eq!(back.last_interaction_at, 1_700_000_000_000);
        assert!(back.is_blocked);

        // Legacy 9-byte payload (written before the block byte) reads as false.
        let legacy = &bytes[..9];
        let old = ContactAdapter::decode("did:plc:x", legacy).unwrap();
        assert!(!old.is_blocked, "legacy record decodes to unblocked");
        assert!(old.is_curated);
    }

    #[test]
    fn conv_settings_round_trips_some_and_none() {
        // Some(timer)
        let some = ConvSettingsRecord {
            conversation_id: "c1".into(),
            expiry_secs: Some(86_400),
        };
        let b = ConvSettingsAdapter::encode(&some);
        assert_eq!(b.len(), 4);
        assert_eq!(
            ConvSettingsAdapter::decode("c1", &b).unwrap().expiry_secs,
            Some(86_400)
        );
        // None (row present, no timer) encodes empty — distinct from a tombstone.
        let none = ConvSettingsRecord {
            conversation_id: "c1".into(),
            expiry_secs: None,
        };
        let b2 = ConvSettingsAdapter::encode(&none);
        assert!(b2.is_empty());
        assert_eq!(
            ConvSettingsAdapter::decode("c1", &b2).unwrap().expiry_secs,
            None
        );
        // A malformed (non-0, non-4) payload is rejected.
        assert!(ConvSettingsAdapter::decode("c1", &[1, 2, 3]).is_err());
    }

    #[test]
    fn contact_profile_round_trips() {
        let p = store::profiles::ContactProfile {
            did: "did:plc:y".into(),
            display_name: "Alice 👋".into(),
            profile_key: vec![9u8; 32],
            fetched_at: Timestamp(42),
        };
        let b = ContactProfileAdapter::encode(&p);
        let back = ContactProfileAdapter::decode("did:plc:y", &b).unwrap();
        assert_eq!(back.did, "did:plc:y");
        assert_eq!(back.display_name, "Alice 👋");
        assert_eq!(back.profile_key, vec![9u8; 32]);
        assert_eq!(back.fetched_at.as_millis(), 42);
    }

    #[test]
    fn default_registry_has_four_distinct_types_and_triggers() {
        let reg = SyncRegistry::default_registry();
        for tag in [
            TYPE_GROUP_KEY,
            TYPE_CONTACT,
            TYPE_CONV_SETTINGS,
            TYPE_CONTACT_PROFILE,
        ] {
            assert!(reg.get(tag).is_some(), "missing adapter for tag {tag}");
        }
        let specs = reg.trigger_specs();
        assert_eq!(specs.len(), 4);
        let tables: std::collections::HashSet<_> =
            specs.iter().map(|s| s.table.clone()).collect();
        assert_eq!(tables.len(), 4, "each synced type watches a distinct table");
    }

    #[tokio::test]
    async fn blanket_bridge_applies_upserts_reads_and_tombstones() {
        let store = store::IdentityStore::open_in_memory().await.unwrap();
        let adapter = ContactAdapter;

        // apply(Some) → decode + upsert into the domain table.
        let rec = ContactRecord {
            did: "did:plc:z".into(),
            is_curated: true,
            last_interaction_at: 100,
            is_blocked: false,
        };
        let payload = ContactAdapter::encode(&rec);
        adapter.apply(&store, "did:plc:z", Some(&payload)).await.unwrap();

        // read() → load + encode, round-tripping through the store.
        let read = adapter.read(&store, "did:plc:z").await.unwrap().unwrap();
        let back = ContactAdapter::decode("did:plc:z", &read).unwrap();
        assert!(back.is_curated);
        assert_eq!(back.last_interaction_at, 100);

        // apply(None) → delete the domain row (tombstone).
        adapter.apply(&store, "did:plc:z", None).await.unwrap();
        assert!(store.load_contact("did:plc:z").await.unwrap().is_none());
        assert!(adapter.read(&store, "did:plc:z").await.unwrap().is_none());
    }
}
