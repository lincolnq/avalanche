//! Database schema definition.
//!
//! The entire schema lives in a single SQL string ([`MIGRATIONS`]) applied as
//! an idempotent batch using `CREATE TABLE IF NOT EXISTS`. This means the same
//! string is safe to run on every database open — on a fresh database it
//! creates all tables; on an existing one it is a no-op.
//!
//! When the schema needs to change, add new `ALTER TABLE` or `CREATE TABLE IF
//! NOT EXISTS` statements to the end of [`MIGRATIONS`] rather than modifying
//! existing statements. The goal is that the string remains idempotent and
//! forward-only so no migration-version tracking is needed at this stage.

/// All schema migrations applied as a single idempotent batch.
/// Tables use `CREATE TABLE IF NOT EXISTS` so this is safe to run on every open.
pub const MIGRATIONS: &str = "
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

-- Local identity key pair and libsignal registration ID.
-- Constrained to one row.
CREATE TABLE IF NOT EXISTS identity_keypair (
    id              INTEGER PRIMARY KEY CHECK (id = 1),
    keypair_bytes   BLOB    NOT NULL,
    registration_id INTEGER NOT NULL
);

-- Trust store: known identity keys for remote addresses.
-- Used by libsignal's IdentityKeyStore to detect key changes.
CREATE TABLE IF NOT EXISTS known_identities (
    address       TEXT NOT NULL PRIMARY KEY,   -- \"name.deviceId\"
    identity_key  BLOB NOT NULL
);

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

-- Local account state: DID and homeserver URL.
-- Constrained to one row.
CREATE TABLE IF NOT EXISTS account (
    id           INTEGER PRIMARY KEY CHECK (id = 1),
    account_id   TEXT    NOT NULL,
    server_url   TEXT    NOT NULL,
    registered_at INTEGER NOT NULL   -- unix millis
);

-- Kyber (post-quantum) prekey pool.
CREATE TABLE IF NOT EXISTS kyber_prekeys (
    id     INTEGER NOT NULL PRIMARY KEY,
    record BLOB    NOT NULL
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

-- Decrypted message history for chat persistence across app restarts.
-- Plaintext is stored encrypted-at-rest via SQLCipher.
CREATE TABLE IF NOT EXISTS message_history (
    id                TEXT    NOT NULL PRIMARY KEY,     -- UUID
    conversation_id   TEXT    NOT NULL,
    sender_did        TEXT    NOT NULL,
    body              TEXT    NOT NULL,
    sent_at           INTEGER NOT NULL,                -- unix millis
    edited_at         INTEGER                          -- unix millis, nullable
);
CREATE INDEX IF NOT EXISTS idx_message_history_conv
    ON message_history (conversation_id, sent_at);
";

/// Migrations that use ALTER TABLE and cannot be expressed idempotently
/// in pure SQL. Applied after [`MIGRATIONS`] on every open.
pub const ALTER_MIGRATIONS: &[&str] = &[
    // Add read_at column for per-message read tracking.
    "ALTER TABLE message_history ADD COLUMN read_at INTEGER",
    // Add delivery_status column for outgoing message status tracking.
    // 0 = sending, 1 = sent, 2 = delivered, 3 = read
    "ALTER TABLE message_history ADD COLUMN delivery_status INTEGER NOT NULL DEFAULT 1",
    // Push notification pseudonym + device token for this device.
    // Constrained to one row via id = 1.
    "CREATE TABLE IF NOT EXISTS push_state (\
        id            INTEGER PRIMARY KEY CHECK (id = 1),\
        pseudonym     TEXT    NOT NULL,\
        device_token  TEXT    NOT NULL,\
        platform      TEXT    NOT NULL,\
        registered_at INTEGER NOT NULL\
    )",
    // P-256 rotation key for DID operations and recovery.
    // Constrained to one row. Private key is stored as SEC1 scalar bytes.
    "CREATE TABLE IF NOT EXISTS rotation_key (\
        id            INTEGER PRIMARY KEY CHECK (id = 1),\
        private_key   BLOB    NOT NULL,\
        public_key    BLOB    NOT NULL\
    )",
    // Local profile state: own profile key + cached display name.
    // Constrained to one row.
    "CREATE TABLE IF NOT EXISTS own_profile (\
        id            INTEGER PRIMARY KEY CHECK (id = 1),\
        profile_key   BLOB    NOT NULL,\
        display_name  TEXT    NOT NULL\
    )",
    // Cached decrypted contact profiles, keyed by DID.
    "CREATE TABLE IF NOT EXISTS contact_profiles (\
        did           TEXT    PRIMARY KEY,\
        display_name  TEXT    NOT NULL,\
        profile_key   BLOB    NOT NULL,\
        fetched_at    INTEGER NOT NULL\
    )",
    // Local device_id assigned to this client (defaults to 1 — single device).
    // Persisted so login/recovery don't have to assume a fixed value.
    "ALTER TABLE account ADD COLUMN device_id INTEGER NOT NULL DEFAULT 1",
];
