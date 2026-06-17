-- Storage service: the durable identity-state store (docs/05-device-data-sync.md §5).
--
-- Each account holds an opaque, client-encrypted key/value store. The server is
-- type-blind: it sees only `record_id` (an HMAC of type+key, see §4) and
-- `ciphertext`, never the plaintext or the record type. It enforces only
-- byte/count quotas (§10) and orders writes for delta pull.
--
-- Two counters per record/account:
--   * `version` — per-record CAS token. A write supplies the version it expects
--     to overwrite; a mismatch is a conflict and is rejected (§5, §9 LWW).
--   * `seq`     — per-account monotonic cursor space. Clients delta-pull
--     everything with `seq > cursor`. Both columns are sourced from the single
--     per-account counter in `storage_seq` (see db/storage.rs), so every applied
--     write gets a fresh, account-unique, monotonically increasing value.

CREATE TABLE storage_items (
    account_id BIGINT      NOT NULL REFERENCES accounts(id),
    record_id  BYTEA       NOT NULL,
    version    BIGINT      NOT NULL,            -- per-record CAS token
    seq        BIGINT      NOT NULL,            -- per-account monotonic; cursor space
    ciphertext BYTEA       NOT NULL,            -- empty for tombstones (deleted = TRUE)
    deleted    BOOLEAN     NOT NULL DEFAULT FALSE,
    byte_len   INTEGER     NOT NULL,            -- ciphertext length, for the quota counter (§10)
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (account_id, record_id)
);

-- Delta-pull scans `WHERE account_id = $1 AND seq > $cursor ORDER BY seq`.
CREATE INDEX storage_items_seq ON storage_items (account_id, seq);

-- Per-account monotonic counter feeding both `version` and `seq`. Bumped in the
-- same transaction as the write via INSERT ... ON CONFLICT DO UPDATE RETURNING,
-- which is atomic without explicit locking.
CREATE TABLE storage_seq (
    account_id BIGINT NOT NULL PRIMARY KEY REFERENCES accounts(id),
    next_seq   BIGINT NOT NULL
);
