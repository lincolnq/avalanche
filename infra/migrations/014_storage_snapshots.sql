-- Storage service: passive-backup snapshots (docs/05-device-data-sync.md §5/§7).
--
-- The identity store is single-authoritative: all live reads/writes go to the
-- account on the identity's discovery server (`/v1/storage/items`). The
-- identity's *other* accounts hold passive, one-way encrypted snapshots of the
-- whole store — write-only backups, never read unless the authoritative account
-- is lost. "Replication" is just "occasionally push a snapshot": one-directional,
-- conflict-free, explicitly not multi-master (§7).
--
-- One whole-store blob per account, opaque to the server (encrypted under the
-- identity storage key, §4). Last-writer-wins on `snapshot_version`: a PUT stores
-- the blob iff its version is strictly newer than the one held, so a stale
-- backup push can never clobber a fresher snapshot.

CREATE TABLE storage_snapshots (
    account_id       BIGINT      NOT NULL PRIMARY KEY REFERENCES accounts(id),
    snapshot_version BIGINT      NOT NULL,  -- LWW token; higher wins
    blob             BYTEA       NOT NULL,  -- whole-store ciphertext (opaque)
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);
