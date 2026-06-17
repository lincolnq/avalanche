//! Database schema definitions for the two split stores.
//!
//! The local store is split into two SQLCipher databases (docs/06):
//!
//! - **device.db** ([`DEVICE_MIGRATIONS`]) — per-device transport crypto and
//!   server-bound caches: Double Ratchet sessions, prekey pools, sender keys,
//!   push state, group credential/param caches, the outbound queue, the
//!   storage-sync *cursor*, and this device's registration (registration_id +
//!   server_url + device_id). Never synced; fully rebuildable.
//! - **identity.db** ([`IDENTITY_MIGRATIONS`]) — durable per-identity state that
//!   is the same on every device: the identity keypair + DID, rotation key,
//!   storage key, contacts, groups (master keys), profiles, conversation
//!   settings, the trust store, the storage-sync sidecar, and (parked here for
//!   now — the event-log split is deferred, docs/06 §12) message history.
//!
//! Each schema is a single idempotent `CREATE TABLE IF NOT EXISTS` batch applied
//! on every open: on a fresh database it creates all tables; on an existing one
//! it is a no-op. Migration of a pre-split single-file database into the two new
//! files is handled separately in [`crate::db`] (file copy + per-side prune),
//! not by replaying ALTERs here.

/// Schema for **device.db** — per-device crypto + server-bound caches.
pub const DEVICE_MIGRATIONS: &str = "
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

-- Session records: one per remote (account, device) pair.
CREATE TABLE IF NOT EXISTS sessions (
    address TEXT NOT NULL PRIMARY KEY,         -- \"name.deviceId\"
    record  BLOB NOT NULL
);

-- One-time prekey pool.
CREATE TABLE IF NOT EXISTS prekeys (
    id     INTEGER NOT NULL PRIMARY KEY,
    record BLOB    NOT NULL
);

-- Signed prekeys (typically one active at a time).
CREATE TABLE IF NOT EXISTS signed_prekeys (
    id     INTEGER NOT NULL PRIMARY KEY,
    record BLOB    NOT NULL
);

-- Kyber (post-quantum) prekey pool.
CREATE TABLE IF NOT EXISTS kyber_prekeys (
    id     INTEGER NOT NULL PRIMARY KEY,
    record BLOB    NOT NULL
);

-- Monotonic next-id cursor for replenishing one-time prekey pools (see the
-- note in app-core's prekey layout). `kind` is 'one_time' or 'kyber'.
CREATE TABLE IF NOT EXISTS prekey_counters (
    kind     TEXT    PRIMARY KEY,
    next_id  INTEGER NOT NULL
);

-- libsignal SenderKeyStore. Each row holds one sender's SenderKeyRecord for
-- one (group-derived) distribution_id. Re-seeded per device on link/recovery.
CREATE TABLE IF NOT EXISTS sender_keys (
    address          TEXT NOT NULL,
    distribution_id  TEXT NOT NULL,
    record           BLOB NOT NULL,
    PRIMARY KEY (address, distribution_id)
);

-- Push notification pseudonym + device token for this device.
CREATE TABLE IF NOT EXISTS push_state (
    id            INTEGER PRIMARY KEY CHECK (id = 1),
    pseudonym     TEXT    NOT NULL,
    device_token  TEXT    NOT NULL,
    platform      TEXT    NOT NULL,
    registered_at INTEGER NOT NULL
);

-- Cached daily zkgroup credential, one per (server_url, did, redemption_time).
CREATE TABLE IF NOT EXISTS group_credentials (
    server_url                       TEXT    NOT NULL,
    did                              TEXT    NOT NULL,
    redemption_time                  INTEGER NOT NULL,
    bytes                            BLOB    NOT NULL,
    sender_cert                      BLOB    NOT NULL,
    sender_cert_expires_at           INTEGER NOT NULL,
    PRIMARY KEY (server_url, did, redemption_time)
);

-- Cached server_params per homeserver. Populated lazily on first use.
CREATE TABLE IF NOT EXISTS group_server_params (
    server_url              TEXT    PRIMARY KEY,
    version                 INTEGER NOT NULL,
    bytes                   BLOB    NOT NULL,
    sender_cert_trust_root  BLOB    NOT NULL,
    fetched_at              INTEGER NOT NULL
);

-- Per-DID record of the last server fetch attempt for a name and its outcome.
-- Per-device fetch throttle cache.
CREATE TABLE IF NOT EXISTS profile_fetch_state (
    did              TEXT    PRIMARY KEY,
    last_attempt_at  INTEGER NOT NULL,
    outcome          INTEGER NOT NULL
);

-- Outbound message queue: encrypted messages pending delivery.
CREATE TABLE IF NOT EXISTS message_queue (
    id                  TEXT    NOT NULL PRIMARY KEY,   -- UUID
    recipient_name      TEXT    NOT NULL,
    recipient_device_id INTEGER NOT NULL,
    ciphertext          BLOB    NOT NULL,
    message_kind        INTEGER NOT NULL,               -- 0 = PreKey, 1 = Whisper
    enqueued_at         INTEGER NOT NULL                -- unix millis
);

-- Single-row cursor: highest server `seq` consumed by a delta pull (docs/05
-- §3.1). Per-(device, server); deliberately kept out of the identity store so
-- it never enters a snapshot (docs/06 §4).
CREATE TABLE IF NOT EXISTS storage_cursor (
    id   INTEGER PRIMARY KEY CHECK (id = 1),
    seq  INTEGER NOT NULL
);

-- This device's registration: the libsignal registration_id (per-device) plus
-- the (server_url, device_id) this account context is bound to. One row.
CREATE TABLE IF NOT EXISTS device_account (
    id              INTEGER PRIMARY KEY CHECK (id = 1),
    server_url      TEXT    NOT NULL,
    device_id       INTEGER NOT NULL,
    registered_at   INTEGER NOT NULL,   -- unix millis
    registration_id INTEGER NOT NULL
);
";

/// Schema for **identity.db** — durable per-identity state (synced; snapshotted).
pub const IDENTITY_MIGRATIONS: &str = "
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

-- Local identity key pair. The long-term key for the DID; the same on every
-- device of the identity (bootstrapped from the recovery blob, docs/06 §5).
-- (registration_id is per-device and lives in device.db's device_account.)
CREATE TABLE IF NOT EXISTS identity_keypair (
    id              INTEGER PRIMARY KEY CHECK (id = 1),
    keypair_bytes   BLOB    NOT NULL
);

-- The identity's DID and when this identity was first established locally.
CREATE TABLE IF NOT EXISTS account_identity (
    id            INTEGER PRIMARY KEY CHECK (id = 1),
    did           TEXT    NOT NULL,
    registered_at INTEGER NOT NULL   -- unix millis
);

-- Trust store: known identity keys for remote addresses. Synced across the
-- identity's devices (docs/06 §12) so a safety-number change flags everywhere.
CREATE TABLE IF NOT EXISTS known_identities (
    address       TEXT NOT NULL PRIMARY KEY,   -- \"name.deviceId\"
    identity_key  BLOB NOT NULL
);

-- P-256 rotation key for DID operations and recovery. Private key is SEC1 bytes.
CREATE TABLE IF NOT EXISTS rotation_key (
    id            INTEGER PRIMARY KEY CHECK (id = 1),
    private_key   BLOB    NOT NULL,
    public_key    BLOB    NOT NULL
);

-- Local profile state: own profile key + cached display name. One row.
CREATE TABLE IF NOT EXISTS own_profile (
    id            INTEGER PRIMARY KEY CHECK (id = 1),
    profile_key   BLOB    NOT NULL,
    display_name  TEXT    NOT NULL
);

-- Cached decrypted contact profiles, keyed by DID.
CREATE TABLE IF NOT EXISTS contact_profiles (
    did           TEXT    PRIMARY KEY,
    display_name  TEXT    NOT NULL,
    profile_key   BLOB    NOT NULL,
    fetched_at    INTEGER NOT NULL
);

-- Per-group state (docs/03-groups.md). group_id and master_key stored
-- base64-url-no-pad. Group *master* keys roam with the identity (docs/05 §1).
CREATE TABLE IF NOT EXISTS groups (
    group_id                    TEXT    PRIMARY KEY,
    master_key                  BLOB    NOT NULL,
    hosting_server_url          TEXT    NOT NULL,
    revision                    INTEGER NOT NULL DEFAULT 0,
    encrypted_state_plaintext   BLOB    NOT NULL DEFAULT x'',
    policy_invite_members_role      INTEGER NOT NULL DEFAULT 1,
    policy_remove_members_role      INTEGER NOT NULL DEFAULT 1,
    policy_modify_title_role        INTEGER NOT NULL DEFAULT 1,
    policy_modify_description_role  INTEGER NOT NULL DEFAULT 1,
    policy_modify_expiry_role       INTEGER NOT NULL DEFAULT 1,
    policy_join_policy              INTEGER NOT NULL DEFAULT 0,
    policy_invite_link_password     BLOB,
    policy_announcement_only        INTEGER NOT NULL DEFAULT 0,
    group_push_pseudonym        BLOB,
    created_at                  INTEGER NOT NULL
);

-- Minimal contact table (docs/52-contacts-and-profiles.md). `is_curated` flips
-- true on any deliberate gesture; `last_interaction_at` drives recency sort.
-- `is_blocked` (docs/12 §2) suppresses a DID and syncs across the identity's
-- devices via the contact storage-sync adapter; `has_pending_request` (docs/52)
-- marks an un-accepted inbound first contact and is local-only (the
-- message-request gate itself is driven by `is_curated`).
-- New columns are also added to pre-existing databases by a guarded
-- `ALTER TABLE ADD COLUMN` in `IdentityStore::migrate`.
CREATE TABLE IF NOT EXISTS contacts (
    did                  TEXT    PRIMARY KEY,
    is_curated           INTEGER NOT NULL DEFAULT 0,
    last_interaction_at  INTEGER NOT NULL DEFAULT 0,
    is_blocked           INTEGER NOT NULL DEFAULT 0,
    has_pending_request  INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_contacts_recency
    ON contacts (last_interaction_at DESC);

-- Per-conversation expiry timer settings (DM and group). expiry_secs = NULL or
-- 0 means no expiry.
CREATE TABLE IF NOT EXISTS conversation_settings (
    conversation_id  TEXT    PRIMARY KEY,
    expiry_secs      INTEGER
);

-- PRF-derived recovery-blob symmetric key, cached after signup or recovery.
CREATE TABLE IF NOT EXISTS recovery_blob_key (
    id            INTEGER PRIMARY KEY CHECK (id = 1),
    key           BLOB    NOT NULL
);

-- Decrypted message history for chat persistence across app restarts. Parked
-- in the identity store while the event-log split is deferred (docs/06 §12).
CREATE TABLE IF NOT EXISTS message_history (
    id                TEXT    NOT NULL PRIMARY KEY,     -- UUID
    conversation_id   TEXT    NOT NULL,
    sender_did        TEXT    NOT NULL,
    body              TEXT    NOT NULL,
    sent_at           INTEGER NOT NULL,                -- unix millis
    edited_at         INTEGER,                         -- unix millis, nullable
    read_at           INTEGER,                         -- NULL = unread
    delivery_status   INTEGER NOT NULL DEFAULT 1,      -- 0=sending..3=read
    edit_count        INTEGER NOT NULL DEFAULT 0,
    deleted_at        INTEGER                          -- non-NULL = tombstone
);
CREATE INDEX IF NOT EXISTS idx_message_history_conv
    ON message_history (conversation_id, sent_at);

-- Prior bodies for the edit-history sheet, keyed by the message's wire identity.
CREATE TABLE IF NOT EXISTS message_revisions (
    conversation_id  TEXT    NOT NULL,
    author_did       TEXT    NOT NULL,
    target_sent_at   INTEGER NOT NULL,
    body             TEXT    NOT NULL,
    replaced_at      INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_message_revisions_target
    ON message_revisions (conversation_id, author_did, target_sent_at, replaced_at);

-- One reaction per (target message, reactor): PK enforces one-per-person.
CREATE TABLE IF NOT EXISTS reactions (
    conversation_id  TEXT    NOT NULL,
    target_author    TEXT    NOT NULL,
    target_sent_at   INTEGER NOT NULL,
    reactor_did      TEXT    NOT NULL,
    emoji            TEXT    NOT NULL,
    reacted_at       INTEGER NOT NULL,
    PRIMARY KEY (conversation_id, target_author, target_sent_at, reactor_did)
);
CREATE INDEX IF NOT EXISTS idx_reactions_conv
    ON reactions (conversation_id);

-- Write-through cache of the server's public account record (display name + bot
-- flag), keyed by DID.
CREATE TABLE IF NOT EXISTS account_info_cache (
    did           TEXT    PRIMARY KEY,
    display_name  TEXT    NOT NULL,
    is_bot        INTEGER NOT NULL,
    fetched_at    INTEGER NOT NULL
);

-- 32-byte identity-level storage key that encrypts durable-state records (docs/05
-- §4). Same across all of the identity's devices. One row.
CREATE TABLE IF NOT EXISTS storage_key_state (
    id           INTEGER PRIMARY KEY CHECK (id = 1),
    storage_key  BLOB    NOT NULL
);

-- Per-record sync bookkeeping sidecar (docs/05 §3.1). Holds NO payload. Lives
-- next to the synced domain tables so the dirty-tracking triggers can write it
-- in the same transaction. The dirty-tracking triggers themselves are installed
-- at open via `IdentityStore::install_sync_triggers` (only when sync is enabled).
CREATE TABLE IF NOT EXISTS storage_sync (
    type         INTEGER NOT NULL,
    logical_key  TEXT    NOT NULL,
    version      INTEGER NOT NULL DEFAULT 0,
    dirty        INTEGER NOT NULL DEFAULT 0,
    deleted      INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (type, logical_key)
);
";

/// Tables that belong to the **device** store. Used by the one-time migration
/// from a pre-split single-file database to drop the device tables from the
/// (reused) identity file. Order is irrelevant (no FKs between them).
pub const DEVICE_TABLES: &[&str] = &[
    "sessions",
    "prekeys",
    "signed_prekeys",
    "kyber_prekeys",
    "prekey_counters",
    "sender_keys",
    "push_state",
    "group_credentials",
    "group_server_params",
    "profile_fetch_state",
    "message_queue",
    "storage_cursor",
];

/// Tables that belong to the **identity** store. Used by the one-time migration
/// to drop the identity tables from the copied device file. The legacy combined
/// schema also had an `account` table (now split into `account_identity` +
/// `device_account`); it is dropped explicitly by the migration.
pub const IDENTITY_TABLES: &[&str] = &[
    "identity_keypair",
    "account_identity",
    "known_identities",
    "rotation_key",
    "own_profile",
    "contact_profiles",
    "groups",
    "contacts",
    "conversation_settings",
    "recovery_blob_key",
    "message_history",
    "message_revisions",
    "reactions",
    "account_info_cache",
    "storage_key_state",
    "storage_sync",
];
