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
    // ── Groups (docs/03-groups.md) ────────────────────────────────────────
    // Per-group state. group_id is the server-visible routing id (32 bytes,
    // derived from master_key). Both stored base64-url-no-pad as TEXT for
    // ergonomics with the URL-safe-no-pad convention server-side.
    //
    // `encrypted_state_plaintext` is the proto::groups::GroupState bytes the
    // client most recently decrypted; cached so the UI can render without a
    // round-trip through libsignal's blob decrypt.
    //
    // `policy_*` columns mirror the server's group_policy so we can render
    // permissions without first decrypting state. They're authoritative on
    // the server; clients trust the server's copy at fetch time and update
    // these columns on every successful fetch.
    "CREATE TABLE IF NOT EXISTS groups (\
        group_id                    TEXT    PRIMARY KEY,\
        master_key                  BLOB    NOT NULL,\
        hosting_server_url          TEXT    NOT NULL,\
        revision                    INTEGER NOT NULL DEFAULT 0,\
        encrypted_state_plaintext   BLOB    NOT NULL DEFAULT x'',\
        policy_invite_members_role      INTEGER NOT NULL DEFAULT 1,\
        policy_remove_members_role      INTEGER NOT NULL DEFAULT 1,\
        policy_modify_title_role        INTEGER NOT NULL DEFAULT 1,\
        policy_modify_description_role  INTEGER NOT NULL DEFAULT 1,\
        policy_modify_expiry_role       INTEGER NOT NULL DEFAULT 1,\
        policy_join_policy              INTEGER NOT NULL DEFAULT 0,\
        policy_invite_link_password     BLOB,\
        policy_announcement_only        INTEGER NOT NULL DEFAULT 0,\
        group_push_pseudonym        BLOB,\
        created_at                  INTEGER NOT NULL\
    )",
    // Cached daily zkgroup credential, one per (server_url, did,
    // redemption_time). `bytes` is the serialized `AuthCredentialWithPniZkc`.
    // `sender_cert` is the libsignal SenderCertificate the server minted
    // alongside the credential — same expiration class, so it lives in the
    // same row. Old rows can be pruned by redemption_time.
    "CREATE TABLE IF NOT EXISTS group_credentials (\
        server_url                       TEXT    NOT NULL,\
        did                              TEXT    NOT NULL,\
        redemption_time                  INTEGER NOT NULL,\
        bytes                            BLOB    NOT NULL,\
        sender_cert                      BLOB    NOT NULL,\
        sender_cert_expires_at           INTEGER NOT NULL,\
        PRIMARY KEY (server_url, did, redemption_time)\
    )",
    // Cached server_params per homeserver. Populated lazily on first use.
    // `sender_cert_trust_root` is the curve25519 pubkey we pin to validate
    // sender certs in the sealed-sender group flow.
    "CREATE TABLE IF NOT EXISTS group_server_params (\
        server_url              TEXT    PRIMARY KEY,\
        version                 INTEGER NOT NULL,\
        bytes                   BLOB    NOT NULL,\
        sender_cert_trust_root  BLOB    NOT NULL,\
        fetched_at              INTEGER NOT NULL\
    )",
    // libsignal SenderKeyStore. Each row holds one sender's SenderKeyRecord
    // for one (group-derived) distribution_id. `address` is the
    // "name.device_id" form (same as `sessions`). `distribution_id` is a
    // 16-byte UUID rendered as canonical hyphenated text — that's what
    // `uuid::Uuid::to_string()` produces and what we store directly.
    "CREATE TABLE IF NOT EXISTS sender_keys (\
        address          TEXT NOT NULL,\
        distribution_id  TEXT NOT NULL,\
        record           BLOB NOT NULL,\
        PRIMARY KEY (address, distribution_id)\
    )",
    // Minimal contact table from docs/52-contacts-and-profiles.md.
    // `is_curated` flips true on any deliberate gesture (sending a DM,
    // inviting to a group, etc.) and is the single source of truth for
    // the People list. `last_interaction_at` drives recency sort.
    // Display name lookups still go through `contact_profiles`.
    "CREATE TABLE IF NOT EXISTS contacts (\
        did                  TEXT    PRIMARY KEY,\
        is_curated           INTEGER NOT NULL DEFAULT 0,\
        last_interaction_at  INTEGER NOT NULL DEFAULT 0\
    )",
    "CREATE INDEX IF NOT EXISTS idx_contacts_recency \
        ON contacts (last_interaction_at DESC)",
    // Per-conversation expiry timer settings (DM and group).
    // expiry_secs = NULL or 0 means no expiry. conversation_id for DMs
    // is the other participant's DID; for groups it is the group_id.
    "CREATE TABLE IF NOT EXISTS conversation_settings (\
        conversation_id  TEXT    PRIMARY KEY,\
        expiry_secs      INTEGER\
    )",
    // PRF-derived recovery-blob symmetric key, cached after signup or
    // recovery. Lets the client re-encrypt + upload an updated blob
    // (e.g. on group join) without re-prompting the passkey, since the
    // user only authenticates with a passkey at account-creation /
    // recovery time. Constrained to one row.
    "CREATE TABLE IF NOT EXISTS recovery_blob_key (\
        id            INTEGER PRIMARY KEY CHECK (id = 1),\
        key           BLOB    NOT NULL\
    )",
    // ── Message editing & deletion (docs/36-message-editing-deletion.md) ──
    // Per-message edit counter (for the human ~10 cap) and tombstone marker.
    // `deleted_at` non-NULL means the message is a FOR_EVERYONE tombstone:
    // body is cleared, reactions dropped, position/sent_at retained.
    "ALTER TABLE message_history ADD COLUMN edit_count INTEGER NOT NULL DEFAULT 0",
    "ALTER TABLE message_history ADD COLUMN deleted_at INTEGER",
    // Prior bodies for the edit-history sheet, keyed by the message's wire
    // identity (conversation_id, author, sent_at) so it survives even if the
    // message_history row's UUID changes. `replaced_at` is when this body was
    // superseded. Bot authors retain no history — nothing is written for them.
    "CREATE TABLE IF NOT EXISTS message_revisions (\
        conversation_id  TEXT    NOT NULL,\
        author_did       TEXT    NOT NULL,\
        target_sent_at   INTEGER NOT NULL,\
        body             TEXT    NOT NULL,\
        replaced_at      INTEGER NOT NULL\
    )",
    "CREATE INDEX IF NOT EXISTS idx_message_revisions_target \
        ON message_revisions (conversation_id, author_did, target_sent_at, replaced_at)",
    // ── Emoji reactions (docs/33-reactions.md) ───────────────────────────
    // One reaction per (target message, reactor): the PK enforces Signal's
    // one-per-person rule. The target message is identified by its wire
    // identity (author, sent_at) within a conversation, NOT by message_history
    // row id, so a reaction can be stored and converge even if it arrives
    // before its target message.
    "CREATE TABLE IF NOT EXISTS reactions (\
        conversation_id  TEXT    NOT NULL,\
        target_author    TEXT    NOT NULL,\
        target_sent_at   INTEGER NOT NULL,\
        reactor_did      TEXT    NOT NULL,\
        emoji            TEXT    NOT NULL,\
        reacted_at       INTEGER NOT NULL,\
        PRIMARY KEY (conversation_id, target_author, target_sent_at, reactor_did)\
    )",
    "CREATE INDEX IF NOT EXISTS idx_reactions_conv \
        ON reactions (conversation_id)",
];
