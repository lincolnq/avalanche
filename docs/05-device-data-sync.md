# Device data sync & the storage service

Status: **partially implemented.** Stages 1 (server) and 2 (client engine + the
first adapter) are built; stages 3–5 are not. See §13 for the per-stage status.
Sections marked **OPEN** are unresolved; **DECIDED** are committed. Spun out of
`docs/04-multi-device.md` §5, which got too big once the durable-state mechanism
was fleshed out.

Background reading:

- `docs/04-multi-device.md` §5 — the channel model this doc's "Durable channel" comes from; the *event* channel (sent transcripts, read marks) stays there.
- `docs/50-identity-auth-recovery.md` — the storage key, rotation key, and recovery blob this chains through.
- `docs/03-groups.md` — group master keys live in the local `groups` table; this doc syncs that table, it does not duplicate it.
- `docs/52-contacts-and-profiles.md` — contacts/profile are consumers of this service.
- `docs/35-attachments.md` — large content (avatars, files) goes to the media path; the store holds only references.

## 1. Scope

How a user's **durable identity state** stays consistent across all of an
identity's devices and survives total device loss. Durable state = current-value
data that isn't a message: contacts + nicknames, blocked list, group master keys,
per-conversation flags (mute/archive/pin/marked-unread), settings, and profile
pointers.

**Scope is the identity (DID), per the terminology in `00`** — not a single
`(identity, server)` account, and not across identities. This state is shared
across all of the identity's devices and *replicated* across its accounts (one
authoritative, the rest passive backups — §7); but separate identities are
deliberately isolated personas that never share durable state (so a block or mute
under one identity does not reach another — see `docs/53-multi-account-ux.md`).
Server-side, each account stores its own copy keyed by `account_id` (§5).

There are two cross-device sync channels (see `04` §5). They are different
mechanisms and this doc owns only the second:


| Channel     | Data                                | Transport                          | Owned by |
| ----------- | ----------------------------------- | ---------------------------------- | -------- |
| Event       | sent transcripts, read/viewed marks | the message queue                  | `04` §5  |
| **Durable** | current-value identity state        | **the storage service (this doc)** | **here** |


Out of scope: the event channel (`04`); large content *bytes* (`35` — the store
holds references, never the bytes); the cryptographic device substrate (`04`
§§1–4).

## 2. Design goals

1. **One local source of truth.** Durable state lives in normal typed SQLCipher
  domain tables (`groups`, `contacts`, …), used directly by feature code. The
   sync layer **never duplicates the payload** — it syncs those tables in place.
2. **Adding a new synced type is trivial.** A domain table + a small adapter + a
  one-line registration, and sync/encryption/conflict/fast-sync/backup/recovery
   all work with no further code. This is the primary ergonomic goal (§3).
3. **Server sees only opaque ciphertext.** No type-awareness; enumeration and
  semantics are client-side.
4. **Tractable sync.** Single authoritative server, per-record last-writer-wins,
  no multi-master, no CRDTs, no vector clocks (§7, §9).
5. **Prompt propagation.** Push nudge + delta pull (§8).
6. **Powers linked devices and recovery** from the same mechanism (§11).

## 3. The core model: domain tables + sync sidecar + adapters

The whole design exists to make goal 2 true. Three pieces:

- **Domain tables** — the operational source of truth, owned by feature code
(e.g. `groups.master_key` lives in the `groups` table and nowhere else).
- **A generic sync engine** (§6) — syncs domain rows to/from the storage service.
It is the *only* code that talks to the server; feature code never does.
- **A thin sidecar table** — per-record sync bookkeeping (version, dirty,
tombstone). **No payload.** The payload is read from / written to the domain
table on demand.
- **An adapter per synced type** — the small glue that tells the engine how to
serialize a row and write it back. This is all a feature author writes.

### 3.1 The sidecar table

```sql
-- bookkeeping only; never holds payload (that's in the domain tables)
CREATE TABLE storage_sync (
  type        INTEGER NOT NULL,   -- TYPE_TAG: which adapter owns this record
  logical_key TEXT    NOT NULL,   -- natural key, e.g. groups.group_id
  version     INTEGER NOT NULL DEFAULT 0,  -- server CAS token last seen
  dirty       INTEGER NOT NULL DEFAULT 0,  -- local change pending push
  deleted     INTEGER NOT NULL DEFAULT 0,  -- tombstone pending push
  PRIMARY KEY (type, logical_key)
);
-- plus a single-row cursor (highest server `seq` consumed)
CREATE TABLE storage_cursor (id INTEGER PRIMARY KEY CHECK (id = 1), seq INTEGER NOT NULL);
```

The opaque server-side `record_id` is recomputable from `(type, logical_key)`
(§4), so the sidecar never stores it.

### 3.2 The adapter

A feature author implements one typed trait per synced type:

```rust
/// One synced record type. Implemented once per domain table that syncs.
trait SyncedType {
    /// Stable, globally-unique, NEVER-reused tag. Part of the record_id.
    const TYPE_TAG: u16;
    type Record;

    /// Natural key (e.g. group_id, contact DID). With TYPE_TAG → record_id.
    fn logical_key(r: &Self::Record) -> String;

    /// Payload codec. Protobuf recommended for forward-compatible fields.
    fn encode(r: &Self::Record) -> Vec<u8>;
    fn decode(bytes: &[u8]) -> Result<Self::Record, StoreError>;

    /// Write-through into the domain table (apply a pulled record).
    fn upsert(store: &Store, r: &Self::Record) -> Result<(), StoreError>;
    fn delete(store: &Store, logical_key: &str) -> Result<(), StoreError>;

    /// Read a record from the domain table (build ciphertext on push).
    fn load(store: &Store, logical_key: &str) -> Result<Option<Self::Record>, StoreError>;
}
```

The engine consumes adapters through an object-safe, byte-oriented view, with a
blanket bridge from `SyncedType` so authors only ever write the typed trait:

```rust
trait SyncAdapter {                               // what the registry stores
    fn type_tag(&self) -> u16;
    /// payload = None means tombstone.
    fn apply(&self, store: &Store, logical_key: &str, payload: Option<&[u8]>) -> Result<(), StoreError>;
    fn read(&self, store: &Store, logical_key: &str) -> Result<Option<Vec<u8>>, StoreError>;
}
impl<T: SyncedType> SyncAdapter for T { /* decode→upsert / load→encode / None→delete */ }
```

### 3.3 The registry

```rust
let mut reg = SyncRegistry::new();
reg.add::<GroupKeySync>();   // TYPE_TAG = 1
reg.add::<ContactSync>();    // TYPE_TAG = 2
reg.add::<SettingsSync>();   // TYPE_TAG = 3
// …
```

The engine routes a pulled record to `reg[type].apply(...)` and, on push, asks
each registered adapter for its dirty rows. `TYPE_TAG`s are assigned from a
central enum so they're never reused (reuse would alias two types onto one
`record_id` space).

### 3.4 Dirty tracking — hands-off via triggers

Feature code must not have to *remember* to mark a row dirty. A per-table trigger
does it automatically, so feature code just writes its domain table as it always
has:

```sql
-- one set per syncable table; TYPE_TAG and key column are the only specifics
CREATE TRIGGER groups_sync_aiu AFTER INSERT ON groups BEGIN
  INSERT INTO storage_sync(type, logical_key, dirty) VALUES (1, NEW.group_id, 1)
  ON CONFLICT(type, logical_key) DO UPDATE SET dirty = 1, deleted = 0;
END;
-- AFTER UPDATE: same body
CREATE TRIGGER groups_sync_ad AFTER DELETE ON groups BEGIN
  INSERT INTO storage_sync(type, logical_key, dirty, deleted) VALUES (1, OLD.group_id, 1, 1)
  ON CONFLICT(type, logical_key) DO UPDATE SET dirty = 1, deleted = 1;
END;
```

The triggers are mechanical (only the table, `TYPE_TAG`, and key column vary), so
they can be generated from the registry rather than hand-written.

> **Why triggers and not write-wrappers?** Triggers can't be forgotten and can't
> be bypassed by a stray `UPDATE`. The cost is one boilerplate migration snippet
> per table, which is generatable. If you'd rather keep DB logic minimal, the
> alternative is routing all domain writes through a helper that touches
> `storage_sync` in the same transaction — same effect, more discipline required.

### 3.5 How to add a new synced type (the payoff)

1. **Domain table** — add it + a migration (or reuse an existing table).
2. **Adapter** — `impl SyncedType` (encode/decode/upsert/delete/load) and pick a
  fresh `TYPE_TAG` from the central enum.
3. **Register + triggers** — `reg.add::<MyTypeSync>()` and add the three
  generated triggers.

Nothing else. CAS, the cursor, conflict resolution, encryption, the push nudge,
backup snapshots, and recovery bootstrap are all handled by the engine. A type
that should *not* roam (Device-local per `04` §5 — theme, biometric lock) simply
has no adapter and no triggers.

## 4. Encryption & opacity

- **Storage key** — a 32-byte **identity-level** key (per DID — the same across
all of the identity's devices and accounts), provisioned alongside the identity
key at link time and carried in the recovery blob (§11). Never sent to any server.
- **Record id** — `record_id = HMAC-SHA256(storage_key, u16_be(TYPE_TAG) || logical_key)[..16]`.
Deterministic, so two devices independently address the same record without a
shared manifest; opaque, so the server learns neither type nor key.
- **Ciphertext envelope** — `version(1) || nonce(12) || AES-256-GCM(storage_key, encode(record))`,
matching the recovery-blob envelope shape. The `TYPE_TAG` is also bound as
associated data so a record can't be replayed under the wrong type.

**Opacity constraint:** because records are opaque, the server can enforce only
byte/count limits (§10), never type-aware ones, and cannot enumerate by type —
enumeration is a local query over the domain tables (which the client fully
mirrors anyway). "List my group keys" is `SELECT … FROM groups`, never a server
call.

## 5. Storage service API (server)

Per-record server-assigned `version` (CAS token) + a per-account monotonic `seq`
(the cursor space). Authoritative server only; backups speak the snapshot
endpoints only (§7).

```http
GET /v1/storage/items?since={cursor}&limit={n}        # delta (or full, since=0) pull
→ 200 { "items": [ { "record_id","version","seq","deleted","ciphertext" } ],
        "next_cursor": <int>, "has_more": <bool> }

PUT /v1/storage/items                                  # batch write, per-item CAS
  { "writes": [ { "record_id","expected_version","deleted","ciphertext" } ] }
→ 200 { "applied":   [ { "record_id","version","seq" } ],
        "conflicts": [ { "record_id","current_version" } ] }   # stale expected_version
```

`expected_version: 0` = create-if-absent. Writes apply independently; conflicts
are returned un-applied and the client re-pulls + retries those.

Snapshot side-channel for the passive backups (§7) — one whole-store blob, LWW on
`snapshot_version`:

```http
PUT /v1/storage/snapshot { "snapshot_version", "blob" }   # stored iff newer
GET /v1/storage/snapshot → 200 { "snapshot_version", "blob" } | 404
```

Server tables:

```sql
CREATE TABLE storage_items (
  account_id BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  record_id  BYTEA  NOT NULL,
  version    BIGINT NOT NULL,            -- per-record CAS token
  seq        BIGINT NOT NULL,            -- per-account monotonic; cursor space
  ciphertext BYTEA  NOT NULL,
  deleted    BOOLEAN NOT NULL DEFAULT FALSE,
  byte_len   INTEGER NOT NULL,           -- for the running quota counter (§10)
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  PRIMARY KEY (account_id, record_id)
);
CREATE INDEX storage_items_seq ON storage_items (account_id, seq);

CREATE TABLE storage_snapshots (
  account_id       BIGINT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
  snapshot_version BIGINT NOT NULL,
  blob             BYTEA  NOT NULL,
  updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

The write handler runs in a `pool.begin()` transaction: CAS-check the row's
`version`, allocate `next_seq` from a per-account counter *in the same txn*,
upsert, enforce quotas (§10), return `(version, seq)`. `GET` is fetchable and
`PUT` writable, so both need rate limiting per the endpoint checklist.

Everything here is keyed by `account_id` — the `(identity, server)` account on
*this* server (per `00`). The identity-scope of the store (§1) is realized by
replicating across the identity's accounts (§7), of which only the authoritative
one is a live read/write target; the others receive snapshots.

## 6. Sync engine (client)

The only component that talks to the storage service. FFI export is sync
(blocks on the global runtime); feature code just mutates domain tables and the
triggers + engine do the rest.

```rust
fn sync(&self) -> Result<(), AppError> {
    // 1. PULL — apply everything newer than our cursor, routed by TYPE_TAG.
    let mut cursor = store.storage_cursor();
    loop {
        let page = client.storage_pull(cursor, 500)?;
        for it in &page.items {
            if it.version <= store.sync_version(it.record_id) { continue; } // LWW
            let (tag, key, payload) = decrypt_and_split(&storage_key, it)?;  // payload None if deleted
            registry[tag].apply(&store, &key, payload.as_deref())?;          // write-through to domain table
            store.set_sync_meta(tag, &key, it.version, /*dirty*/ false, it.deleted);
        }
        cursor = page.next_cursor;
        if !page.has_more { break }
    }
    store.set_storage_cursor(cursor);

    // 2. PUSH — every dirty row, built on demand from its domain table.
    let writes = store.dirty_records().iter().map(|m| {
        let payload = if m.deleted { None } else { registry[m.type].read(&store, &m.logical_key)? };
        build_write(&storage_key, m, payload)   // record_id + expected_version = m.version
    }).collect();
    let res = client.storage_push(writes)?;
    for a in res.applied   { store.set_sync_meta_clean(a.record_id, a.version); }
    for c in res.conflicts { /* leave dirty; next pull re-merges then we retry */ }
    Ok(())
}
```

Writes are local-first: a domain mutation sets `dirty` via trigger immediately;
`sync()` reconciles asynchronously. A separate periodic task serializes the full
local set and `PUT /snapshot`s it to each backup server (§7).

### 6.1 What schedules a push

The trigger and the wakeup are **two separate jobs, both global** — neither needs
any per-write-path code. Feature code just writes its domain table.


| Job                       | Mechanism                                | Wired                        |
| ------------------------- | ---------------------------------------- | ---------------------------- |
| Mark *which* row is dirty | per-table trigger (§3.4)                 | once per table (generatable) |
| Wake the sync task        | rusqlite `commit_hook` on the connection | once, at store open          |


- **Durability (the trigger's only job).** `dirty = 1` is a *persistent record of
intent*, written in the same transaction as the data — crash-atomic. If the app
dies before pushing, the bit is still there on next launch and the next `sync()`
flushes it. The trigger does no networking and wakes nothing.
- **Scheduling (the commit hook).** A single `commit_hook`, registered once when
the store opens, fires on every commit regardless of which path did the write. It
sets an in-memory "dirty exists" flag and pokes a `tokio::sync::Notify`; a
background task coalesces the burst and calls `sync()` once — the push-side mirror
of the §8 pull coalescing. (The engine's own pull write-backs also fire the hook;
since `apply` clears `dirty`, the woken `sync()` finds nothing to push and no-ops.)
- **Safety-net poll.** A periodic tick (and on foreground / WebSocket reconnect)
runs `sync()` whenever any dirty row exists. This makes the system self-healing:
a missed `Notify` still gets flushed, because the durable dirty bit outlives it.

```
feature code writes domain table
  → trigger sets dirty=1            (durable intent — survives crash)
  → commit_hook pokes sync task     (scheduling — best-effort, fast, debounced)
  → sync() reads dirty_records(), pushes, clears dirty on `applied`
  → safety-net poll catches anything the poke missed
```

Pushing inline from the write path is deliberately avoided: it would block the FFI
call on the network, fail offline, and lose crash-durability. Decoupling through
the dirty bit gives the local-first property — the write always succeeds locally,
the push happens whenever connectivity allows.

## 7. Where it lives — one authoritative account, passive backups

**DECIDED. The deliberate choice is to NOT build multi-master replication.**

The store is identity-scoped (§1) but physically hosted on the identity's
accounts — one authoritative, the rest passive:

- **On the identity's own accounts only** (the servers where the DID is
registered), never a server you don't control, always ciphertext. A seized
store server yields nothing readable.
- **One authoritative copy** — the account on your default/discovery server
(`servers[0]`), same home-and-proxy pattern as the profile blob. **All device
reads/writes go here**, so there is no cross-account divergence to reconcile.
- **The identity's other accounts (its remaining `servers`) hold passive, one-way
encrypted snapshots** — write-only backups, never read unless the authoritative
account is lost.
"Replication" is just "occasionally push a snapshot": one-directional,
conflict-free, explicitly not multi-master.

This stays tractable because the data cooperates (§9). **Cost accepted:** seizure
of the authoritative server pauses live sync until you promote a backup, and you
may lose the last few minutes of mutable changes; recovery still works from the
latest snapshot + the recovery blob.

**Backing storage:** the homeserver may persist on managed object storage
(S3/R2/GCS), as it does media — an infra detail. A *consumer* cloud (iCloud/Drive)
is **not** the substrate (it would re-centralize on a subpoenable party and leak
DID↔platform metadata, against the threat model).

## 8. Fast sync (push nudge + delta pull)

§5–6 alone would be poll-only. The fast path reuses the existing per-device
WebSocket (`routes/websocket.rs`): **push carries the signal, pull carries the
data.**

1. Device A writes → server bumps `seq` to N.
2. Server nudges A's *other* connected devices: `{"storage_changed": true, "high_seq": N}` — metadata only.
3. Each device delta-pulls `since={cursor}`.

**Push is a latency optimization; correctness is the cursor pull.** Missed nudges
are harmless (idempotent pull); offline devices catch up on reconnect via the
same `since={cursor}`. **Coalescing is client-side, server does no debounce:** the
high-water `seq` makes redundant nudges near-free, and a device that gets nudges
mid-pull sets a "resync" flag and pulls once more when done — any burst collapses
to ~one extra pull. Backgrounded apps get a silent push (APNs/FCM) or sync on next
foreground. All of this is single-authoritative-server; backups are never in the
fast path.

## 9. Conflict model

**Per-record last-writer-wins**, safe here because the data cooperates:

- **Single-user, low-write, low-contention** — two of your own devices writing the
same record in the same instant is rare, and "two setting toggles, keep the
newer" is a benign resolution.
- **Records are independent** — no cross-record invariants, so no multi-record
atomicity and no transaction spanning records.
- **Most records are immutable or monotonic** — a group master key is added on
join and tombstoned on leave, never mutated; only flags/settings actually
mutate, where LWW is harmless.

So no CRDTs/OT/vector clocks. CAS on the per-record `version` + LWW on apply is
sufficient.

## 10. Limits / quotas

The governing limit is **snapshot/recovery cost** (the whole store is blobbed to
backups and pulled on recovery), not raw storage — and large content never enters
the store (it goes to the media path; the store holds only references), so records
are intrinsically small.


| Cap                       | Start    | Why                                                                         |
| ------------------------- | -------- | --------------------------------------------------------------------------- |
| **Total bytes / account** | ~4–8 MB  | Governing limit — snapshot + recovery cost. Binds first.                    |
| **Per-record ciphertext** | ~8 KB    | Real records are <1 KB; stops a record becoming a file.                     |
| **Record count**          | ~10–25 k | Secondary guard; contacts + per-conversation flags can reach low thousands. |


Enforced server-side at `PUT` via a per-account running byte+count counter
updated in the write txn (`byte_len` column). Because records are opaque, limits
are byte/count only — any semantic ("max N contacts") limits live client-side.

## 11. Storage key, recovery blob, and bootstrap

The storage key chains the recovery blob to the store (see `04` §5.6 for the full
argument). Summary:

- **Recovery blob** (passkey-encrypted, occasional) shrinks to a keyring: identity
key, rotation derivation, server list, and **the storage key** — *not* the
durable data itself.
- **Storage key** (shared across devices) encrypts the live store.

Both bootstrap paths converge on "get the storage key, then pull the store":

- **Linking a device:** identity key + storage key arrive over the provisioning
channel (`04` §4) → engine pulls `since=0` → write-through fills every domain
table → fully operational. (No special "fetch group keys" step; the full pull is
cheap.)
- **Total-loss recovery:** passkey → decrypt recovery blob → identity key +
storage key → pull `since=0` (or promote a backup snapshot) → same.

**Coupling to note:** because group master keys moved out of the recovery blob and
into the store, the snapshot-to-backups path (§7) is now load-bearing for
total-loss recovery — it provides what the blob's inline group keys used to. The
snapshots are one-way and conflict-free, so this doesn't reintroduce multi-master.

**Recovery-blob size cap (consequence of the above).** Once group keys leave the
blob, the recovery blob is a near-**constant keyring** (identity key, rotation
derivation, server list, storage key) — a few KB, no longer growing with group
count. That makes a tight, *explicit* byte cap on `PUT /v1/recovery` both safe and
worth adding: the `GET /v1/recovery/{did}` read path is **unauthenticated** (opaque
ciphertext, fetchable by anyone who knows the DID), so an unbounded blob is a
public storage/bandwidth-abuse vector — the same concern as §10's quotas but on a
public path. Today there is no intentional cap: only a rate limit on `PUT` plus
axum's accidental 2 MB `Json` body default. Recommended: a deliberate
`MAX_RECOVERY_BLOB` constant (32–64 KB — comfortably above a keyring, far below
2 MB), enforced in the handler. **Sequencing:** the tight cap must land *with or
after* the migration that moves group keys into the store (this doc), since
today's blob still inlines a 32-byte `master_key` per group and would blow a 64 KB
cap for a heavy-membership user; until then, use a transitional cap sized for
worst-case membership.

## 12. End-to-end walkthroughs

- **Add a contact.** Feature code inserts a row into `contacts`. Trigger marks
`storage_sync(type=CONTACT, key=did)` dirty. `sync()` reads the row via the
adapter, encrypts, CAS-pushes; the server bumps `seq` and nudges your other
devices; they delta-pull and `ContactSync::upsert` writes the row into *their*
`contacts`. No feature code on the read side.
- **Join a group.** `groups.rs` writes `master_key` into the `groups` table (as it
does today). Trigger marks it dirty; `sync()` pushes it as a `GROUP_KEY` record;
other devices pull and `GroupKeySync::upsert` lands it in their `groups` table.
The key bytes exist in exactly one local table per device — `groups` — never a
duplicate.
- **Link a new device.** §11 — pull `since=0`, every adapter fills its domain
table, operational with no per-type bootstrap code.
- **Recover after losing everything.** §11 — passkey → storage key → pull.

## 13. Status & open questions

- **DECIDED:** domain-tables + sidecar + adapters (no payload duplication);
single-authoritative + passive backups; per-record LWW; client-side coalescing;
storage-key chaining; **trigger marks dirty, a single `commit_hook` schedules
the push, periodic poll is the safety net — zero per-write-path code (§6.1)**.
- **OPEN:** the `SyncRegistry` / trigger generation ergonomics (hand-written vs.
generated triggers); exact `TYPE_TAG` registry location; snapshot cadence;
whether the snapshot is a serialized item-set or an independent re-encode.
### 13.1 Implementation status (by stage)

**Stage 1 — server `storage_items` + `/v1/storage/items` — DONE.**

- `infra/migrations/013_storage_items.sql` — `storage_items` (PK `(account_id,
record_id)`, `version`/`seq`/`byte_len`/`deleted`), index `storage_items_seq`,
and a per-account `storage_seq` counter table.
- `core/crates/server/src/db/storage.rs` — `StorageItem`, `PutOutcome::{Applied,
Conflict}`, `account_usage` (running byte+count quota), `pull` (cursor query),
`alloc_seq` (atomic upsert), `put_item` (CAS under `SELECT … FOR UPDATE`).
- `core/crates/server/src/routes/storage.rs` — authenticated, account-scoped
`pull_items` (GET) + `put_items` (PUT) with the §10 caps as consts
(`MAX_RECORD_BYTES` 8 KB, `MAX_TOTAL_BYTES` 8 MB, `MAX_RECORD_COUNT` 25 000,
pull limit clamped to 1000, ≤500 writes/request). Tombstones store their sealed
header verbatim (§4) rather than blanked ciphertext.
- Rate limits: `ACTION_STORAGE_PULL`/`PUSH` in
`core/crates/server/src/middleware/rate_limit.rs`. `delete_account` now also
purges `storage_items` + `storage_seq` (`db/accounts.rs`).
- Tests: 8 db + 4 http storage tests (`server/tests/{db,http}_tests.rs`).

**Stage 2 — client sidecar + engine + group-key adapter — DONE.**

- Sidecar schema (`storage_sync`, `storage_cursor`, `storage_key_state`) and the
three `groups` dirty-tracking triggers (TYPE_TAG 1) in
`core/crates/store/src/schema.rs`; sidecar accessors in
`core/crates/store/src/storage_sync.rs`.
- `core/crates/net/src/lib.rs` — `storage_pull`/`storage_push` + wire types.
- `core/crates/app-core/src/storage_sync.rs` — record crypto (§4: `record_id`
HMAC, `seal`/`open` with the `tag‖key_len‖logical_key‖payload` plaintext
layout), `SyncAdapter` trait + `SyncRegistry`, `GroupKeyAdapter`, and the
`sync`/`pull`/`push` engine.
- Storage-key provisioning: generated at account creation, carried in the
recovery blob (`storage_key` field added to `proto/recovery.proto`;
`build_recovery_blob` in `app-core/src/recovery.rs`), and restored on recover
(`app-core/src/lib.rs`).
- FFI: `sync_storage` (sync export) + `sync_storage_async`
(`app-core/src/lib.rs`); iOS picks it up via the no-op default in
`mobile/.../AppCoreProtocol+Defaults.swift`.
- Tests: store sidecar tests (`store/tests/store_tests.rs`) + the push→pull
restore e2e (`app-core/tests/e2e_storage.rs`).

**Stages 3–5 — NOT YET BUILT.**

- (3) Trigger *generation* from the registry (triggers are still hand-written),
the ergonomic `SyncedType`→`SyncAdapter` blanket bridge (§3.2), and more
adapters (contacts, settings).
- (4) Snapshot endpoints + backup push (§5/§7) — `storage_snapshots` table and
`PUT`/`GET /v1/storage/snapshot` are **not** implemented; only `/items` exists.
This path is load-bearing for total-loss recovery (§11), so recovery currently
relies on the authoritative account's live items only.
- (5) Fast-sync nudge on the WebSocket (§8) — sync is poll/explicit-call only;
no `storage_changed` push yet.

### 13.2 Known gaps / deferred

- The §11 total-loss recovery path is not covered by an automated e2e test
(`recover_from_blob` is an FFI constructor, so a nested `block_on` can't run
inside an async test; needs a `recover_*_async` harness).
- The §11 `MAX_RECOVERY_BLOB` cap is not yet enforced; the blob now carries the
storage key but group keys still inline a `master_key` per group, so the tight
cap must wait until group-key sync is the sole path (see §11 sequencing note).

