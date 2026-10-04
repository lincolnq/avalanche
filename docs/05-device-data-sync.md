# 05 — Device data sync & the storage service

> **Status:** Partial — the storage service (server `/v1/storage/items`), the client engine, four synced types, trigger-based dirty tracking, the commit-hook scheduler, and the WebSocket fast-sync nudge are built. Passive backups/snapshots are parked. Conflict handling and unknown-type handling have data-loss bugs (Known gaps).
> **Last verified against code:** 2026-10-03

## Summary

How an identity's **durable state** (contacts, blocked flags, group master keys, per-conversation settings, contact profiles) stays consistent across its devices and survives total device loss. Durable state lives in ordinary SQLCipher domain tables; a generic engine syncs those tables to an opaque per-record store on the identity's discovery homeserver. Adding a synced type means writing one small adapter.

The event channel (sent transcripts, read marks) is a different mechanism and lives in `04` §5. Section numbers below are cited from code and are stable.

## Known gaps

- **Conflicts drop the local edit.** On a push conflict the row stays dirty, but the next `pull` sees a newer server version, applies the server's payload over the local domain row, and clears `dirty` (`app-core/src/storage_sync.rs` `pull`, the `set_sync_meta(..., false, ...)` after `adapter.apply`). The pending local change is silently discarded, despite the comment in `push` that says it will be retried. Example: a contact blocked offline on one device can be undone by an unrelated edit from another device. Semantics today are "first write to reach the server wins", not last-writer-wins.
- **Records of an unknown type are lost for good.** When a client sees a type tag it has no adapter for, it records the version and advances the cursor without keeping the payload (`pull`, the `None =>` arm). After the user upgrades to a build that knows the type, those records are never re-applied.
- **No rollback protection.** The record envelope binds the type tag and logical key (checked via `record_id`) but not the version. A malicious or seized server can serve an older ciphertext under a newer version number (for example, re-instating an unblocked contact or a left group's key) and the client accepts it.
- **The trust store is not synced.** `06` §12 decided `known_identities` should roam; there is no adapter for it (only tags 1–4 exist).
- **The recovery blob still inlines group master keys** and has no size cap (§11). `GET /v1/recovery/{did}` is unauthenticated, so blob size reveals roughly how many groups a user is in (`09`).
- **Total-loss recovery is not e2e-tested** (§13.2).

## 1. Scope

Durable state is **identity-scoped** (per DID): shared across the identity's devices, never across identities. Separate identities are deliberately isolated personas, so a block under one does not reach another (`53`). Server-side, each account stores its own copy keyed by `account_id` (§5).

| Channel | Data | Transport | Owned by |
|---|---|---|---|
| Event | sent transcripts, read marks | the message queue | `04` §5 |
| **Durable** | current-value identity state | **the storage service (this doc)** | **here** |

Out of scope: the event channel (`04`); large content bytes (`35` — the store holds references, never bytes); the device crypto substrate (`04` §§1–4).

## 2. Design goals

1. **One local source of truth.** Durable state lives in typed SQLCipher domain tables; the sync layer never duplicates the payload.
2. **Adding a synced type is trivial**: domain table + small adapter + one-line registration.
3. **Server sees only opaque ciphertext.** No type awareness.
4. **Tractable sync.** Single authoritative server, per-record versions, no CRDTs or vector clocks (§7, §9).
5. **Prompt propagation.** Push nudge + delta pull (§8).
6. **Powers linking and recovery** from the same mechanism (§11).

## 3. The core model: domain tables + sync sidecar + adapters

**Built.** Pieces:

- **Domain tables** — the operational source of truth, owned by feature code (e.g. `groups.master_key`).
- **A generic sync engine** (§6) — the only code that talks to the storage service.
- **A thin sidecar table** — per-record version, dirty, and tombstone bits. **No payload.**
- **An adapter per synced type** — tells the engine how to serialize a row and write it back.

### 3.1 The sidecar table

```sql
CREATE TABLE storage_sync (
  type        INTEGER NOT NULL,   -- TYPE_TAG: which adapter owns this record
  logical_key TEXT    NOT NULL,   -- natural key, e.g. groups.group_id
  version     INTEGER NOT NULL DEFAULT 0,  -- server CAS token last seen
  dirty       INTEGER NOT NULL DEFAULT 0,  -- local change pending push
  deleted     INTEGER NOT NULL DEFAULT 0,  -- tombstone pending push
  PRIMARY KEY (type, logical_key)
);
CREATE TABLE storage_cursor (id INTEGER PRIMARY KEY CHECK (id = 1), seq INTEGER NOT NULL);
```

The sidecar lives in identity.db; the cursor is per-device (`06` §4). The opaque `record_id` is recomputable from `(type, logical_key)` (§4).

### 3.2 The adapter

Authors implement one typed trait per synced type (`app-core/src/storage_sync.rs`, `SyncedType`):

```rust
trait SyncedType {
    const TYPE_TAG: u16;               // stable, never reused
    type Record;
    fn encode(r: &Self::Record) -> Vec<u8>;
    fn decode(logical_key: &str, bytes: &[u8]) -> Result<Self::Record, StoreError>;
    fn upsert(store: &Store, r: &Self::Record) -> Result<(), StoreError>;
    fn delete(store: &Store, logical_key: &str) -> Result<(), StoreError>;
    fn load(store: &Store, logical_key: &str) -> Result<Option<Self::Record>, StoreError>;
}
```

A blanket `impl<T: SyncedType> SyncAdapter for T` gives the engine an object-safe, byte-oriented view (`apply`, `read`).

### 3.3 The registry

`SyncRegistry` holds the adapters. Built types (module constants in `app-core/src/storage_sync.rs`):

| Tag | Adapter | Domain table |
|---|---|---|
| 1 | `GroupKeyAdapter` | `groups` (master key + hosting server) |
| 2 | `ContactAdapter` | `contacts` (curation, block, nickname flags) |
| 3 | `ConvSettingsAdapter` | `conversation_settings` |
| 4 | `ContactProfileAdapter` | `contact_profiles` (name, profile key) |

### 3.4 Dirty tracking — hands-off via triggers

**Built.** Per-table `AFTER INSERT/UPDATE/DELETE` triggers mark the sidecar row dirty (or tombstoned) in the same transaction as the domain write. They are generated from the registry (`SyncRegistry::trigger_specs` → `Store::install_sync_triggers`) and installed at account open, only when a storage key is present. Triggers can't be forgotten or bypassed by a stray `UPDATE`.

### 3.5 How to add a new synced type

1. Domain table (and migration), or reuse an existing one.
2. `impl SyncedType` with a fresh `TYPE_TAG`.
3. Register it. Triggers are generated.

CAS, cursor, encryption, push nudge, and recovery bootstrap are handled by the engine.

## 4. Encryption & opacity

**Built.**

- **Storage key** — 32 bytes, identity-level, provisioned at link time and carried in the recovery blob (§11). Never sent to a server. **Its presence is the opt-in signal**: humans get one at creation, bots don't, and with no key the engine, triggers, and scheduler all no-op.
- **Record id** — `HMAC-SHA256(storage_key, u16_be(TYPE_TAG) || logical_key)[..16]`. Deterministic, so devices address the same record without a manifest; opaque to the server.
- **Ciphertext envelope** — `version(1) || nonce(12) || AES-256-GCM(storage_key, u16 tag || u16 key_len || logical_key || payload)`. The tag and key travel inside the ciphertext (not as associated data); on pull the client recomputes `record_id` from them and rejects a mismatch, which stops a record being replayed under a different type or key. The **version is not bound** (Known gaps).

The server can enforce only byte and count limits (§10). Enumeration is a local query.

## 5. Storage service API (server)

**Built** (`server/src/routes/storage.rs`, `db/storage.rs`, migration `013`). Per-record server-assigned `version` (CAS token) and a per-account monotonic `seq` (cursor space).

```http
GET /v1/storage/items?since={cursor}&limit={n}
→ 200 { "items": [ { "record_id","version","seq","deleted","ciphertext" } ],
        "next_cursor": <int>, "has_more": <bool> }

PUT /v1/storage/items
  { "writes": [ { "record_id","expected_version","deleted","ciphertext" } ] }
→ 200 { "applied":   [ { "record_id","version","seq" } ],
        "conflicts": [ { "record_id","current_version" } ] }
```

`expected_version: 0` = create-if-absent. Writes apply independently. The write handler runs in a transaction: CAS-check under `SELECT … FOR UPDATE`, allocate `seq` from a per-account counter, upsert, enforce quotas. Both endpoints are authenticated, account-scoped, and rate-limited (`ACTION_STORAGE_PULL`/`PUSH`).

Snapshot endpoints (`PUT`/`GET /v1/storage/snapshot`, migration `014`, `MAX_SNAPSHOT_BYTES` 12 MB) are built server-side but have no client (§7).

## 6. Sync engine (client)

**Built** (`app-core/src/storage_sync.rs`, `sync` / `pull` / `push`). Pull applies everything newer than the cursor, routed by tag; push builds ciphertext for every dirty row from its domain table and CAS-writes it. FFI: `sync_storage` (sync) + `sync_storage_async`.

### 6.1 What schedules a push

**Built.** Two jobs, neither needing per-write-path code:

| Job | Mechanism |
|---|---|
| Mark *which* row is dirty | generated trigger (§3.4) — durable intent, crash-atomic |
| Wake the sync task | one rusqlite `commit_hook` that pokes a `Notify` |

`storage_sync::run_scheduler` debounces bursts and runs `sync()`, with a 60 s safety-net poll and a sync on WebSocket reconnect. A settled sync commits nothing, so the loop quiesces. Pushing inline from the write path is avoided: it would block on the network, fail offline, and lose crash-durability.

## 7. Where it lives — one authoritative account

**Decided: not multi-master.** All device reads and writes go to the account on the identity's discovery server (`servers[0]`). Always ciphertext, always on the identity's own accounts, never on a consumer cloud.

**Parked: passive backups.** The original design had the identity's other accounts hold one-way snapshots for promotion if the authoritative server is lost. The client `build_snapshot`/`restore_snapshot` code is written and tested but commented out, and no snapshot client methods exist. The reason it is parked: a snapshot is a different kind of server storage from `/items` (no `seq`, no per-record CAS), so a backup cannot be promoted without a restore-then-reseed sequence. Do not re-enable without settling the replacement (see Speculative, which would remove snapshots entirely).

In practice multi-server identities are not built yet either (`53`, `06` §9), so today there is only ever one account per identity and the question is moot until that lands. **Cost accepted meanwhile:** if the discovery server is lost, durable state that is not in the recovery blob is lost with it.

## 8. Fast sync (push nudge + delta pull)

**Built.** After a `PUT /v1/storage/items` applies, the server sends a `StorageChangedNotification` WS frame to the account's *other* connected devices (never the writer); the client receive loop (`app-core/src/connection.rs`) delta-pulls. Push carries the signal, pull carries the data. Missed nudges are harmless; coalescing is client-side.

## 9. Conflict model

**Intended:** per-record last-writer-wins, safe because the data cooperates: single user, low contention, independent records, mostly immutable or monotonic values. No CRDTs, no OT, no vector clocks.

**As built:** the server's version wins on conflict and the losing local edit is discarded (Known gaps). The fix is small: on pull, if the local row is dirty, keep the local payload and re-push against the new version, instead of applying the remote payload. True LWW across devices needs a client-assigned clock (Speculative).

## 10. Limits / quotas

**Built.** Enforced in the write transaction via a running byte/count counter: `MAX_RECORD_BYTES` 8 KB, `MAX_TOTAL_BYTES` 8 MB, `MAX_RECORD_COUNT` 25 000; pull limit clamped to 1000; ≤500 writes per request. Semantic limits ("max N contacts") are client-side.

## 11. Storage key, recovery blob, and bootstrap

**Built.** The storage key chains the recovery blob to the store. Both bootstrap paths converge on "get the storage key, then pull `since=0`":

- **Linking:** identity, rotation, and storage keys arrive over the provisioning channel (`04` §4) → full pull fills every domain table.
- **Total-loss recovery:** passkey → recovery blob → identity key + storage key → full pull from the discovery server.

The recovery blob today contains the identity keypair, the server list, the profile key and display name, the storage key, **and every group master key** (`app-core/src/recovery.rs` `build_recovery_blob`). Group keys are in both the blob and the store.

**Planned:** drop group keys from the blob once the store is the sole path, leaving a near-constant keyring, then enforce a deliberate `MAX_RECOVERY_BLOB` (32–64 KB) on `PUT /v1/recovery`. Today the only bound is axum's 2 MB default body limit.

## 12. End-to-end walkthroughs

- **Add a contact.** Feature code inserts into `contacts`; the trigger marks it dirty; the scheduler pushes; the server nudges your other devices; they pull and `ContactAdapter` writes their `contacts` row.
- **Join a group.** `groups.rs` writes `master_key` into `groups`; it syncs as a tag-1 record; other devices' reconciler then registers pseudonyms and seeds sender keys (`04` §6).
- **Link a new device.** §11.
- **Recover after losing everything.** §11.

## 13. Status & open questions

**Decided:** domain tables + sidecar + adapters; single authoritative server; trigger marks dirty, one `commit_hook` schedules, periodic poll is the safety net; triggers generated from the registry; storage-key presence is the opt-in (bots opt out).

**Open:** whether to fix conflicts locally (§9) or move to client-assigned versions (Speculative); what replaces passive backups (§7).

### 13.1 Implementation status (by stage)

| Stage | What | Status |
|---|---|---|
| 1 | Server `storage_items` + `/v1/storage/items`, quotas, rate limits, account-deletion purge | Built |
| 2 | Client sidecar, engine, group-key adapter, storage-key provisioning | Built |
| 3 | `SyncedType` bridge, trigger generation, contact/settings/profile adapters, commit-hook scheduler, bot opt-out | Built |
| 4 | Snapshot endpoints (server) | Built |
| 4 | Snapshot client / backup push | Parked |
| 5 | WebSocket fast-sync nudge | Built |

### 13.2 Known gaps / deferred

- Total-loss recovery (§11) has no automated e2e test: `recover_from_blob` is a sync FFI constructor that can't run inside an async test, and it needs a live PLC entry. Needs a `recover_*_async` harness and a PLC stub.
- `MAX_RECOVERY_BLOB` not enforced (§11).
- See also Known gaps at the top.

## Planned

- Keep dirty local edits on pull and re-push (§9).
- Keep unknown-type payloads (store the ciphertext, re-open when an adapter appears) instead of discarding them.
- Bind the record version into the sealed payload, or reject version regressions client-side, to stop server rollback.
- Add a trust-store adapter (`known_identities`).
- Remove group keys from the recovery blob and cap it (§11).

## Speculative

- **Every server a dumb mirror.** Have the client assign each record version (a hybrid logical clock) and seal it inside the ciphertext. That gives true cross-device LWW, makes rollback detectable, and lets every server the identity uses hold identical record sets. Promotion becomes a no-op, which removes the snapshot type and the parked §7 design entirely. Fits with `50` §Proposed and the client-side federation proposal in `13`.

## Rationale and rejected alternatives

- **Multi-master replication, CRDTs, vector clocks (rejected).** The data is single-user and low-contention; per-record LWW is enough.
- **A consumer cloud (iCloud/Drive) as the substrate (rejected).** Re-centralizes on a subpoenable party and leaks DID↔platform metadata.
- **Write-path helpers instead of triggers (rejected).** Same effect but relies on discipline; triggers can't be bypassed.
- **Pushing inline from the write path (rejected).** Blocks on the network, fails offline, loses crash durability.
- **Storing payload in the sidecar (rejected).** Duplicates data and drifts from the domain table.
